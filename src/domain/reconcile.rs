use crate::domain::model::{
    HarnessKind, McpServerConfig, normalize_mcp_server_name, validate_mcp_server_name,
};
use std::collections::{BTreeMap, BTreeSet};

/// Type of conflict detected during startup or live-reload reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    /// Same server name has different definitions or enabled states across harnesses.
    DefinitionDrift,
    /// Two or more server names differ only by ASCII letter case (e.g., `"Context7"` vs `"context7"`).
    CaseCollision { alt_names: Vec<String> },
}

/// A deduplicated definition variant shared by one or more harnesses during a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictVariant {
    pub name: String,
    pub sources: Vec<String>,
    pub config: McpServerConfig,
}

impl ConflictVariant {
    pub fn sources_label(&self) -> String {
        self.sources.join(", ")
    }
}

/// A conflict requiring user decision before overwriting diverging harness entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConflict {
    pub name: String,
    pub kind: ConflictKind,
    pub variants: Vec<ConflictVariant>,
    pub maga: Option<McpServerConfig>,
}

impl McpConflict {
    pub fn primary_name(&self) -> &str {
        &self.name
    }

    pub fn alternate_names(&self) -> &[String] {
        match &self.kind {
            ConflictKind::CaseCollision { alt_names } => alt_names.as_slice(),
            ConflictKind::DefinitionDrift => &[],
        }
    }

    pub fn alternate_name(&self) -> Option<&str> {
        self.alternate_names().first().map(String::as_str)
    }

    pub fn matches_name(&self, candidate: &str) -> bool {
        self.name == candidate
            || self.alternate_names().iter().any(|a| a == candidate)
            || self.variants.iter().any(|v| v.name == candidate)
    }

    /// Returns a list of human-readable field names that differ across `variants`.
    pub fn differing_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        let has_name_diff = matches!(self.kind, ConflictKind::CaseCollision { .. })
            || self
                .variants
                .first()
                .is_some_and(|first| self.variants.iter().any(|v| v.name != first.name));
        if has_name_diff {
            fields.push("name (casing)");
        }

        let Some(first) = self.variants.first().map(|v| &v.config) else {
            return fields;
        };

        if self
            .variants
            .iter()
            .any(|v| v.config.enabled != first.enabled)
        {
            fields.push("enabled");
        }
        if self
            .variants
            .iter()
            .any(|v| v.config.transport != first.transport)
        {
            fields.push("transport");
        }
        if self
            .variants
            .iter()
            .any(|v| v.config.command != first.command)
        {
            fields.push("command");
        }
        if self.variants.iter().any(|v| v.config.args != first.args) {
            fields.push("args");
        }
        if self.variants.iter().any(|v| v.config.env != first.env) {
            fields.push("env");
        }
        if self.variants.iter().any(|v| v.config.url != first.url) {
            fields.push("url");
        }
        if self
            .variants
            .iter()
            .any(|v| v.config.headers != first.headers)
        {
            fields.push("headers");
        }
        let non_empty_descs: BTreeSet<&str> = self
            .variants
            .iter()
            .filter_map(|v| v.config.description.as_deref())
            .collect();
        if non_empty_descs.len() > 1 {
            fields.push("description");
        }

        fields
    }
}

/// Resolution choice applied by the user to a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    KeepVariant(usize),
    Custom {
        final_name: String,
        config: McpServerConfig,
    },
}

/// Result of reconciling `mcp.toml` and all enabled harness configurations.
#[derive(Debug, Clone, Default)]
pub struct ReconcileOutcome {
    pub canonical: BTreeMap<String, McpServerConfig>,
    pub conflicts: Vec<McpConflict>,
    pub imported_count: usize,
}

impl ReconcileOutcome {
    /// Applies a resolution for the conflict at `index`, updating `canonical` and removing any case-collision duplicate keys.
    pub fn resolve(&mut self, index: usize, resolution: ConflictResolution) -> Result<(), String> {
        if index >= self.conflicts.len() {
            return Err("Invalid conflict index.".to_string());
        }
        let conflict = self.conflicts.remove(index);
        let fallback_desc = conflict.maga.as_ref().and_then(|m| m.description.clone());

        self.canonical.remove(&conflict.name);
        for alt in conflict.alternate_names() {
            self.canonical.remove(alt);
        }
        for variant in &conflict.variants {
            self.canonical.remove(&variant.name);
        }

        match resolution {
            ConflictResolution::KeepVariant(variant_idx) => {
                let Some(chosen) = conflict.variants.get(variant_idx) else {
                    return Err(format!("Invalid variant index {variant_idx}."));
                };
                let mut chosen_cfg = chosen.config.clone();
                if chosen_cfg.description.is_none() {
                    chosen_cfg.description = fallback_desc;
                }
                self.canonical.insert(chosen.name.clone(), chosen_cfg);
            }
            ConflictResolution::Custom {
                final_name,
                mut config,
            } => {
                validate_mcp_server_name(&final_name)?;
                config.normalize_description();
                config.validate()?;
                self.canonical.insert(final_name, config);
            }
        }

        Ok(())
    }
}

/// Returns `true` if `harness_map` and `canonical_map` have the same keys and every entry matches
/// via `McpServerConfig::same_harness_state` (ignoring `description`).
pub fn harness_maps_match(
    harness_map: &BTreeMap<String, McpServerConfig>,
    canonical_map: &BTreeMap<String, McpServerConfig>,
) -> bool {
    harness_map.len() == canonical_map.len()
        && harness_map.iter().all(|(k, h_cfg)| {
            canonical_map
                .get(k)
                .is_some_and(|c_cfg| h_cfg.same_harness_state(c_cfg))
        })
}

fn normalize_server_map(
    map: &BTreeMap<String, McpServerConfig>,
) -> BTreeMap<String, McpServerConfig> {
    let mut out = BTreeMap::new();
    let mut spaced = Vec::new();
    for (k, v) in map {
        let norm = normalize_mcp_server_name(k);
        if norm.is_empty() {
            continue;
        }
        let mut cfg = v.clone();
        cfg.normalize_description();
        if &norm == k {
            out.insert(norm, cfg);
        } else {
            spaced.push((norm, cfg));
        }
    }
    for (norm, v) in spaced {
        out.entry(norm).or_insert(v);
    }
    out
}

fn build_grouped_variants(entries: Vec<(String, String, McpServerConfig)>) -> Vec<ConflictVariant> {
    let mut variants: Vec<ConflictVariant> = Vec::new();
    for (entry_name, source_label, cfg) in entries {
        if let Some(existing) = variants
            .iter_mut()
            .find(|v| v.name == entry_name && v.config.same_harness_state(&cfg))
        {
            if existing.config.description.is_none() && cfg.description.is_some() {
                existing.config.description = cfg.description;
            }
            if !existing.sources.contains(&source_label) {
                existing.sources.push(source_label);
            }
        } else {
            variants.push(ConflictVariant {
                name: entry_name,
                sources: vec![source_label],
                config: cfg,
            });
        }
    }
    variants
}

/// Reconciles `maga` (`mcp.toml`) and all enabled `harnesses`.
pub fn reconcile_harnesses(
    maga: &BTreeMap<String, McpServerConfig>,
    harnesses: &[(HarnessKind, BTreeMap<String, McpServerConfig>)],
) -> ReconcileOutcome {
    let norm_maga = normalize_server_map(maga);
    let norm_harnesses: Vec<(HarnessKind, BTreeMap<String, McpServerConfig>)> = harnesses
        .iter()
        .map(|(k, m)| (*k, normalize_server_map(m)))
        .collect();
    let maga = &norm_maga;
    let harnesses = norm_harnesses.as_slice();

    let mut all_names = BTreeSet::new();
    for k in maga.keys() {
        all_names.insert(k.clone());
    }
    for (_, h_map) in harnesses {
        for k in h_map.keys() {
            all_names.insert(k.clone());
        }
    }

    // Group names by ASCII lowercase to detect case collisions first
    let mut by_lower: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &all_names {
        by_lower
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(name.clone());
    }

    let mut canonical = maga.clone();
    let mut conflicts = Vec::new();
    let mut imported_count = 0;

    for (_lower, group) in by_lower {
        if group.len() > 1 {
            let group_desc = group
                .iter()
                .find_map(|k| maga.get(k).and_then(|c| c.description.clone()));
            let mut raw_entries: Vec<(String, String, McpServerConfig)> = Vec::new();
            for candidate in &group {
                let mut found_in_harness = false;
                for (kind, h_map) in harnesses {
                    if let Some(cfg) = h_map.get(candidate) {
                        let mut c_with_desc = cfg.clone();
                        if c_with_desc.description.is_none() {
                            c_with_desc.description = maga
                                .get(candidate)
                                .and_then(|m| m.description.clone())
                                .or_else(|| group_desc.clone());
                        }
                        raw_entries.push((
                            candidate.clone(),
                            kind.short_name().to_string(),
                            c_with_desc,
                        ));
                        found_in_harness = true;
                    }
                }
                if !found_in_harness && let Some(cfg) = maga.get(candidate) {
                    raw_entries.push((candidate.clone(), "maga".to_string(), cfg.clone()));
                }
            }

            let variants = build_grouped_variants(raw_entries);
            let primary_name = variants
                .first()
                .map(|v| v.name.clone())
                .unwrap_or_else(|| group[0].clone());
            let alt_names: Vec<String> = group
                .iter()
                .filter(|n| **n != primary_name)
                .cloned()
                .collect();

            if let Some(first_var) = variants.first() {
                canonical
                    .entry(primary_name.clone())
                    .or_insert_with(|| first_var.config.clone());
            }

            let maga_cfg = group.iter().find_map(|k| maga.get(k).cloned());

            conflicts.push(McpConflict {
                name: primary_name,
                kind: ConflictKind::CaseCollision { alt_names },
                variants,
                maga: maga_cfg,
            });
            continue;
        }

        let name = &group[0];
        let m_cfg = maga.get(name);
        let present: Vec<(HarnessKind, &McpServerConfig)> = harnesses
            .iter()
            .filter_map(|(k, map)| map.get(name).map(|c| (*k, c)))
            .collect();

        match m_cfg {
            None => {
                if present.is_empty() {
                    continue;
                }
                let first_cfg = present[0].1;
                let all_match = present.iter().all(|(_, c)| c.same_harness_state(first_cfg));
                canonical.insert(name.clone(), first_cfg.clone());
                if all_match {
                    imported_count += 1;
                } else {
                    let entries = present
                        .into_iter()
                        .map(|(k, c)| (name.clone(), k.short_name().to_string(), c.clone()))
                        .collect();
                    let variants = build_grouped_variants(entries);
                    conflicts.push(McpConflict {
                        name: name.clone(),
                        kind: ConflictKind::DefinitionDrift,
                        variants,
                        maga: None,
                    });
                }
            }
            Some(m) => {
                if present.is_empty() {
                    continue;
                }
                let all_match_m = present.iter().all(|(_, c)| c.same_harness_state(m));
                if all_match_m {
                    continue;
                }

                let first_h = present[0].1;
                let all_harnesses_present =
                    present.len() == harnesses.len() && harnesses.len() >= 2;
                let all_harnesses_agree =
                    present.iter().all(|(_, c)| c.same_harness_state(first_h));

                if all_harnesses_present && all_harnesses_agree && !first_h.same_harness_state(m) {
                    // All enabled harnesses agree on a newer state than mcp.toml; preserve maga description
                    let mut adopted = first_h.clone();
                    if adopted.description.is_none() {
                        adopted.description = m.description.clone();
                    }
                    canonical.insert(name.clone(), adopted);
                    continue;
                }

                let mut entries: Vec<(String, String, McpServerConfig)> = Vec::new();
                for (kind, map) in harnesses {
                    if let Some(c) = map.get(name) {
                        let mut c_with_desc = c.clone();
                        if c_with_desc.description.is_none() {
                            c_with_desc.description = m.description.clone();
                        }
                        entries.push((name.clone(), kind.short_name().to_string(), c_with_desc));
                    } else {
                        entries.push((name.clone(), kind.short_name().to_string(), m.clone()));
                    }
                }
                if !entries.iter().any(|(_, _, c)| c.same_harness_state(m)) {
                    entries.push((name.clone(), "maga".to_string(), m.clone()));
                }

                let variants = build_grouped_variants(entries);
                if variants.len() > 1 {
                    conflicts.push(McpConflict {
                        name: name.clone(),
                        kind: ConflictKind::DefinitionDrift,
                        variants,
                        maga: Some(m.clone()),
                    });
                }
            }
        }
    }

    ReconcileOutcome {
        canonical,
        conflicts,
        imported_count,
    }
}

fn opt_same_harness_state(a: Option<&McpServerConfig>, b: Option<&McpServerConfig>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.same_harness_state(y),
        _ => false,
    }
}

/// Reconciles a live reload from disk against the previously known in-memory canonical snapshot
/// across all enabled harnesses.
pub fn reconcile_live_reload_harnesses(
    prev_canonical: &BTreeMap<String, McpServerConfig>,
    disk_maga: &BTreeMap<String, McpServerConfig>,
    disk_harnesses: &[(HarnessKind, BTreeMap<String, McpServerConfig>)],
) -> ReconcileOutcome {
    let mut adj_maga = normalize_server_map(disk_maga);
    let mut adj_harnesses: Vec<(HarnessKind, BTreeMap<String, McpServerConfig>)> = disk_harnesses
        .iter()
        .map(|(k, m)| (*k, normalize_server_map(m)))
        .collect();

    let mut all_names = BTreeSet::new();
    for k in prev_canonical.keys().chain(adj_maga.keys()) {
        all_names.insert(k.clone());
    }
    for (_, h_map) in &adj_harnesses {
        for k in h_map.keys() {
            all_names.insert(k.clone());
        }
    }

    let mut by_lower: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &all_names {
        by_lower
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(name.clone());
    }

    let mut updated_count = 0usize;

    for group in by_lower.values() {
        if group.len() > 1 {
            // Leave case collisions for `reconcile_harnesses` to flag
            continue;
        }
        let name = &group[0];
        let prev = prev_canonical.get(name);
        let m_val = adj_maga.get(name).cloned();
        let m_changed = m_val.as_ref() != prev;

        let fallback_desc = m_val
            .as_ref()
            .and_then(|m| m.description.clone())
            .or_else(|| prev.and_then(|p| p.description.clone()));

        let mut target: Option<Option<McpServerConfig>> = None;
        let mut conflict = false;

        if m_changed {
            target = Some(m_val.clone());
        }

        for (_, h_map) in &adj_harnesses {
            let h_val = h_map.get(name).cloned().map(|mut cfg| {
                if cfg.description.is_none() {
                    cfg.description = fallback_desc.clone();
                }
                cfg
            });
            let h_changed = !opt_same_harness_state(h_val.as_ref(), prev);
            if h_changed {
                match &target {
                    None => target = Some(h_val),
                    Some(existing) if opt_same_harness_state(existing.as_ref(), h_val.as_ref()) => {
                    }
                    Some(_) => {
                        conflict = true;
                    }
                }
            }
        }

        if !conflict && let Some(winner) = target {
            updated_count += 1;
            match winner {
                Some(cfg) => {
                    adj_maga.insert(name.clone(), cfg.clone());
                    for (_, h_map) in &mut adj_harnesses {
                        h_map.insert(name.clone(), cfg.clone());
                    }
                }
                None => {
                    adj_maga.remove(name);
                    for (_, h_map) in &mut adj_harnesses {
                        h_map.remove(name);
                    }
                }
            }
        }
    }

    let mut outcome = reconcile_harnesses(&adj_maga, &adj_harnesses);
    outcome.imported_count += updated_count;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_untracked_and_detects_multi_harness_drift_and_case_collision_with_grouped_variants()
    {
        let maga = BTreeMap::new();
        let mut agy = BTreeMap::new();
        let mut claude = BTreeMap::new();
        let mut copilot = BTreeMap::new();
        let mut grok = BTreeMap::new();
        let mut opencode = BTreeMap::new();

        // 1. Untracked in Grok only -> auto-import across all 5 harnesses
        grok.insert(
            "filesystem".to_string(),
            McpServerConfig::new_stdio("npx", vec!["filesystem".to_string()]),
        );

        // 2. Identical in Agy, Claude, Copilot, OpenCode -> auto-import without conflict
        let svelte = McpServerConfig::new_http("https://mcp.svelte.dev/mcp");
        agy.insert("Svelte".to_string(), svelte.clone());
        claude.insert("Svelte".to_string(), svelte.clone());
        copilot.insert("Svelte".to_string(), svelte.clone());
        opencode.insert("Svelte".to_string(), svelte);

        // 3. Drift on "sqlite-mcp": Agy + Copilot have v1, Claude + OpenCode have v2 (with APP_ENV=local)
        let sqlite_v1 = McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]);
        let mut sqlite_v2 = sqlite_v1.clone();
        sqlite_v2
            .env
            .insert("APP_ENV".to_string(), "local".to_string());
        agy.insert("sqlite-mcp".to_string(), sqlite_v1.clone());
        copilot.insert("sqlite-mcp".to_string(), sqlite_v1);
        claude.insert("sqlite-mcp".to_string(), sqlite_v2.clone());
        opencode.insert("sqlite-mcp".to_string(), sqlite_v2.clone());

        // 4. Case collision ("Context7" in Copilot/OpenCode vs "context7" in Agy/Claude)
        let ctx7 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        agy.insert("context7".to_string(), ctx7.clone());
        claude.insert("context7".to_string(), ctx7.clone());
        copilot.insert("Context7".to_string(), ctx7.clone());
        opencode.insert("Context7".to_string(), ctx7);

        let harnesses = vec![
            (HarnessKind::Antigravity, agy),
            (HarnessKind::Claude, claude),
            (HarnessKind::Copilot, copilot),
            (HarnessKind::Grok, grok),
            (HarnessKind::OpenCode, opencode),
        ];

        let mut outcome = reconcile_harnesses(&maga, &harnesses);
        assert_eq!(outcome.imported_count, 2); // filesystem + Svelte
        assert!(outcome.canonical.contains_key("filesystem"));
        assert!(outcome.canonical.contains_key("Svelte"));
        assert_eq!(outcome.conflicts.len(), 2); // context7/Context7 + sqlite-mcp

        let sqlite_idx = outcome
            .conflicts
            .iter()
            .position(|c| c.name == "sqlite-mcp")
            .expect("sqlite-mcp conflict");
        let sqlite_conflict = &outcome.conflicts[sqlite_idx];
        assert_eq!(sqlite_conflict.variants.len(), 2);
        assert_eq!(sqlite_conflict.variants[0].sources_label(), "Agy, Copilot");
        assert_eq!(
            sqlite_conflict.variants[1].sources_label(),
            "Claude, OpenCode"
        );

        // Resolve sqlite-mcp in favor of variant 1 (Claude, OpenCode)
        outcome
            .resolve(sqlite_idx, ConflictResolution::KeepVariant(1))
            .expect("resolve sqlite-mcp variant 1");
        assert_eq!(outcome.canonical["sqlite-mcp"], sqlite_v2);

        // Resolve Context7/context7 case collision in favor of variant 1 ("context7" from Agy, Claude)
        let ctx_conflict = &outcome.conflicts[0];
        assert_eq!(ctx_conflict.variants.len(), 2);
        let lower_variant_idx = ctx_conflict
            .variants
            .iter()
            .position(|v| v.name == "context7")
            .expect("context7 variant");
        outcome
            .resolve(0, ConflictResolution::KeepVariant(lower_variant_idx))
            .expect("resolve case collision");
        assert!(outcome.canonical.contains_key("context7"));
        assert!(!outcome.canonical.contains_key("Context7"));
        assert!(outcome.conflicts.is_empty());
    }

    #[test]
    fn resolved_conflicts_sync_out_to_all_five_target_configs() {
        use crate::domain::storage::{LoadedConfigs, StoragePaths};
        use tempfile::tempdir;

        let dir = tempdir().expect("create tempdir");
        let paths = StoragePaths::in_dir(dir.path());

        let mut loaded = LoadedConfigs::default();
        loaded.agy.servers.insert(
            "sqlite-mcp".to_string(),
            McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]),
        );
        let mut claude_sqlite =
            McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]);
        claude_sqlite
            .env
            .insert("APP_ENV".to_string(), "staging".to_string());
        loaded
            .claude
            .servers
            .insert("sqlite-mcp".to_string(), claude_sqlite);

        let harnesses = loaded.enabled_harness_servers();
        let mut outcome = reconcile_harnesses(&loaded.mcp.mcps, &harnesses);
        assert_eq!(outcome.conflicts.len(), 1);

        let mut merged = McpServerConfig::new_stdio("/bin/sqlite-mcp", vec!["mcp".to_string()]);
        merged
            .env
            .insert("APP_ENV".to_string(), "production".to_string());
        outcome
            .resolve(
                0,
                ConflictResolution::Custom {
                    final_name: "sqlite-mcp".to_string(),
                    config: merged.clone(),
                },
            )
            .expect("resolve custom");

        loaded
            .save_all(&paths, &outcome.canonical)
            .expect("save all target configs");

        let reloaded = LoadedConfigs::load(&paths).expect("reload all target configs");
        assert!(paths.config_toml.exists());
        assert!(paths.mcp_toml.exists());
        assert_eq!(reloaded.mcp.mcps["sqlite-mcp"], merged);
        assert_eq!(reloaded.agy.servers["sqlite-mcp"], merged);
        assert_eq!(reloaded.claude.servers["sqlite-mcp"], merged);
        assert_eq!(reloaded.copilot.servers["sqlite-mcp"], merged);
        assert_eq!(reloaded.grok.servers["sqlite-mcp"], merged);
        assert_eq!(reloaded.opencode.servers["sqlite-mcp"], merged);
    }

    #[test]
    fn live_reload_adopts_single_harness_edit_and_flags_multi_harness_conflict() {
        let mut prev = BTreeMap::new();
        prev.insert(
            "context7".to_string(),
            McpServerConfig::new_http("https://mcp.context7.com/mcp"),
        );

        let disk_maga = prev.clone();
        let disk_agy = prev.clone();
        let mut disk_claude = prev.clone();
        let disk_copilot = prev.clone();
        let disk_grok = prev.clone();
        let mut disk_opencode = prev.clone();

        // 1. OpenCode toggles "context7" off and adds "new-oc-mcp" while the other 4 stay at `prev`
        disk_opencode.get_mut("context7").unwrap().enabled = false;
        disk_opencode.insert(
            "new-oc-mcp".to_string(),
            McpServerConfig::new_stdio("npx", vec!["-y".to_string(), "new-mcp".to_string()]),
        );

        let harnesses = vec![
            (HarnessKind::Antigravity, disk_agy.clone()),
            (HarnessKind::Claude, disk_claude.clone()),
            (HarnessKind::Copilot, disk_copilot.clone()),
            (HarnessKind::Grok, disk_grok.clone()),
            (HarnessKind::OpenCode, disk_opencode.clone()),
        ];

        let outcome = reconcile_live_reload_harnesses(&prev, &disk_maga, &harnesses);
        assert!(outcome.conflicts.is_empty());
        assert!(!outcome.canonical["context7"].enabled);
        assert!(outcome.canonical.contains_key("new-oc-mcp"));
        assert!(outcome.imported_count >= 2);

        // 2. Both Claude and OpenCode modify "context7" to different URLs relative to `prev` -> conflict
        disk_claude.get_mut("context7").unwrap().url =
            Some("https://claude.context7.com/mcp".to_string());
        disk_opencode.get_mut("context7").unwrap().url =
            Some("https://opencode.context7.com/mcp".to_string());

        let conflict_harnesses = vec![
            (HarnessKind::Antigravity, disk_agy),
            (HarnessKind::Claude, disk_claude),
            (HarnessKind::Copilot, disk_copilot),
            (HarnessKind::Grok, disk_grok),
            (HarnessKind::OpenCode, disk_opencode),
        ];

        let conflict_outcome =
            reconcile_live_reload_harnesses(&prev, &disk_maga, &conflict_harnesses);
        assert_eq!(conflict_outcome.conflicts.len(), 1);
        assert_eq!(conflict_outcome.conflicts[0].name, "context7");
    }

    #[test]
    fn normalizes_spaced_server_names_during_reconciliation_and_rejects_spaced_custom_resolutions()
    {
        let maga = BTreeMap::new();
        let mut agy = BTreeMap::new();
        let mut claude = BTreeMap::new();

        agy.insert(
            "Astro Docs".to_string(),
            McpServerConfig::new_http("https://mcp.docs.astro.build/mcp"),
        );
        claude.insert(
            "astro-docs".to_string(),
            McpServerConfig::new_http("https://mcp.docs.astro.build/mcp"),
        );

        let harnesses = vec![
            (HarnessKind::Antigravity, agy),
            (HarnessKind::Claude, claude),
        ];

        let mut outcome = reconcile_harnesses(&maga, &harnesses);
        assert!(!outcome.canonical.contains_key("Astro Docs"));
        // "Astro-Docs" vs "astro-docs" is flagged as a case collision
        assert_eq!(outcome.conflicts.len(), 1);
        assert!(matches!(
            outcome.conflicts[0].kind,
            ConflictKind::CaseCollision { .. }
        ));

        // Attempting to resolve with a spaced custom name fails validation
        let bad_res = outcome.resolve(
            0,
            ConflictResolution::Custom {
                final_name: "Astro Docs".to_string(),
                config: McpServerConfig::new_http("https://mcp.docs.astro.build/mcp"),
            },
        );
        assert!(bad_res.is_err());
    }

    #[test]
    fn preserves_maga_description_across_startup_and_live_reload_without_false_conflicts() {
        let mut maga = BTreeMap::new();
        let mut ctx7 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        ctx7.description = Some("Upstash Context7 documentation MCP".to_string());
        maga.insert("context7".to_string(), ctx7);

        let harness_ctx7 = McpServerConfig::new_http("https://mcp.context7.com/mcp");
        let mut agy = BTreeMap::new();
        agy.insert("context7".to_string(), harness_ctx7.clone());
        let mut claude = BTreeMap::new();
        claude.insert("context7".to_string(), harness_ctx7);

        let harnesses = vec![
            (HarnessKind::Antigravity, agy.clone()),
            (HarnessKind::Claude, claude.clone()),
        ];

        // 1. Startup reconciliation: harnesses have no description, maga has description -> 0 conflicts, description kept
        let outcome = reconcile_harnesses(&maga, &harnesses);
        assert!(outcome.conflicts.is_empty());
        assert_eq!(
            outcome.canonical["context7"].description.as_deref(),
            Some("Upstash Context7 documentation MCP")
        );
        assert!(harness_maps_match(&agy, &outcome.canonical));

        // 2. Startup when all harnesses agree on disabled state while maga is enabled -> adopts disabled state AND keeps description
        let mut agy_off = agy.clone();
        agy_off.get_mut("context7").unwrap().enabled = false;
        let mut claude_off = claude.clone();
        claude_off.get_mut("context7").unwrap().enabled = false;
        let off_harnesses = vec![
            (HarnessKind::Antigravity, agy_off),
            (HarnessKind::Claude, claude_off),
        ];
        let adopted_outcome = reconcile_harnesses(&maga, &off_harnesses);
        assert!(adopted_outcome.conflicts.is_empty());
        assert!(!adopted_outcome.canonical["context7"].enabled);
        assert_eq!(
            adopted_outcome.canonical["context7"].description.as_deref(),
            Some("Upstash Context7 documentation MCP")
        );

        // 3. Live reload: Claude toggles context7 off on disk -> adopts disabled state and preserves maga description
        let prev = outcome.canonical.clone();
        let disk_maga = maga.clone();
        let mut disk_claude = claude;
        disk_claude.get_mut("context7").unwrap().enabled = false;
        let live_harnesses = vec![
            (HarnessKind::Antigravity, agy),
            (HarnessKind::Claude, disk_claude),
        ];
        let live_outcome = reconcile_live_reload_harnesses(&prev, &disk_maga, &live_harnesses);
        assert!(live_outcome.conflicts.is_empty());
        assert!(!live_outcome.canonical["context7"].enabled);
        assert_eq!(
            live_outcome.canonical["context7"].description.as_deref(),
            Some("Upstash Context7 documentation MCP")
        );
    }
}
