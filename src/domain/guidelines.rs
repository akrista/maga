use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuidelineHarness {
    Claude,
    Gemini,
    Copilot,
    OpenCode,
    Grok,
    Pi,
}

impl fmt::Display for GuidelineHarness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "Claude Code"),
            Self::Gemini => write!(f, "Antigravity / Gemini"),
            Self::Copilot => write!(f, "GitHub Copilot"),
            Self::OpenCode => write!(f, "OpenCode"),
            Self::Grok => write!(f, "Grok CLI"),
            Self::Pi => write!(f, "Pi Agent"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuidelineTarget {
    pub harness: GuidelineHarness,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GuidelineStatus {
    pub harness: GuidelineHarness,
    pub path: PathBuf,
    pub exists: bool,
    pub is_symlink: bool,
    pub matches_canonical: bool,
}

#[derive(Debug, Clone)]
pub struct GuidelineSyncResult {
    pub harness: GuidelineHarness,
    pub path: PathBuf,
    pub action: String,
    pub success: bool,
}

pub fn home_dir() -> Result<PathBuf, String> {
    std::env::home_dir().ok_or_else(|| "Could not determine $HOME directory.".to_string())
}

pub fn canonical_guidelines_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    Ok(home.join(".agents").join("AGENTS.md"))
}

pub fn target_guidelines() -> Result<Vec<GuidelineTarget>, String> {
    let home = home_dir()?;
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));

    Ok(vec![
        GuidelineTarget {
            harness: GuidelineHarness::Claude,
            path: home.join(".claude").join("CLAUDE.md"),
        },
        GuidelineTarget {
            harness: GuidelineHarness::Gemini,
            path: home.join(".gemini").join("GEMINI.md"),
        },
        GuidelineTarget {
            harness: GuidelineHarness::Copilot,
            path: home.join(".copilot").join("copilot-instructions.md"),
        },
        GuidelineTarget {
            harness: GuidelineHarness::OpenCode,
            path: config_root.join("opencode").join("AGENTS.md"),
        },
        GuidelineTarget {
            harness: GuidelineHarness::Grok,
            path: home.join(".grok").join("AGENTS.md"),
        },
        GuidelineTarget {
            harness: GuidelineHarness::Pi,
            path: home.join(".pi").join("agent").join("AGENTS.md"),
        },
    ])
}

pub fn check_guidelines_status() -> Result<Vec<GuidelineStatus>, String> {
    let canonical = canonical_guidelines_path()?;
    let canonical_bytes = if canonical.exists() {
        fs::read(&canonical).ok()
    } else {
        None
    };

    let targets = target_guidelines()?;
    let mut statuses = Vec::new();

    for target in targets {
        let meta = fs::symlink_metadata(&target.path);
        let exists = meta.is_ok();
        let is_symlink = meta.map(|m| m.file_type().is_symlink()).unwrap_or(false);

        let matches_canonical = if let Some(ref c_bytes) = canonical_bytes {
            if let Ok(t_bytes) = fs::read(&target.path) {
                &t_bytes == c_bytes
            } else {
                false
            }
        } else {
            false
        };

        statuses.push(GuidelineStatus {
            harness: target.harness,
            path: target.path,
            exists,
            is_symlink,
            matches_canonical,
        });
    }

    Ok(statuses)
}

pub fn create_link(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    if dst.exists() || fs::symlink_metadata(dst).is_ok() {
        let _ = fs::remove_file(dst);
    }

    #[cfg(windows)]
    {
        // Try symbolic link first
        if std::os::windows::fs::symlink_file(src, dst).is_ok() {
            return Ok(());
        }
        // Fallback to hard link
        if fs::hard_link(src, dst).is_ok() {
            return Ok(());
        }
        // Fallback to copy if cross-volume or restricted
        fs::copy(src, dst).map(|_| ())
    }

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(src, dst).is_ok() {
            return Ok(());
        }
        if fs::hard_link(src, dst).is_ok() {
            return Ok(());
        }
        fs::copy(src, dst).map(|_| ())
    }

    #[cfg(not(any(windows, unix)))]
    {
        fs::copy(src, dst).map(|_| ())
    }
}

pub const DEFAULT_MAX_GUIDELINE_LINES: usize = 300;
pub const DEFAULT_MAX_GUIDELINE_CHARS: usize = 15_000;
pub const REFS_BLOCK_START: &str = "<!-- maga:refs:start -->";
pub const REFS_BLOCK_END: &str = "<!-- maga:refs:end -->";

/// Metrics describing the length and prompt-budget health of an AI guidelines file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidelineMetrics {
    pub line_count: usize,
    pub char_count: usize,
    pub exceeds_recommended_budget: bool,
}

impl GuidelineMetrics {
    pub fn compute(content: &str) -> Self {
        let line_count = content.lines().count();
        let char_count = content.chars().count();
        let exceeds_recommended_budget = line_count > DEFAULT_MAX_GUIDELINE_LINES
            || char_count > DEFAULT_MAX_GUIDELINE_CHARS;

        Self {
            line_count,
            char_count,
            exceeds_recommended_budget,
        }
    }
}

/// Injects or idempotently updates a delimited reference block in the guidelines content.
pub fn inject_or_replace_refs_block(existing_content: &str, refs_content: &str) -> String {
    let trimmed_refs = refs_content.trim();
    if trimmed_refs.is_empty() {
        if let Some(start_idx) = existing_content.find(REFS_BLOCK_START) {
            if let Some(end_rel) = existing_content[start_idx..].find(REFS_BLOCK_END) {
                let end_idx = start_idx + end_rel + REFS_BLOCK_END.len();
                let mut res = String::new();
                res.push_str(&existing_content[..start_idx].trim_end());
                let tail = existing_content[end_idx..].trim_start();
                if !tail.is_empty() {
                    res.push_str("\n\n");
                    res.push_str(tail);
                }
                res.push('\n');
                return res;
            }
        }
        return existing_content.to_string();
    }

    let block = format!("{REFS_BLOCK_START}\n{trimmed_refs}\n{REFS_BLOCK_END}");

    if let Some(start_idx) = existing_content.find(REFS_BLOCK_START) {
        if let Some(end_rel) = existing_content[start_idx..].find(REFS_BLOCK_END) {
            let end_idx = start_idx + end_rel + REFS_BLOCK_END.len();
            let mut result = String::new();
            result.push_str(&existing_content[..start_idx]);
            result.push_str(&block);
            result.push_str(&existing_content[end_idx..]);
            return result;
        }
    }

    let mut result = existing_content.trim_end().to_string();
    if !result.is_empty() {
        result.push_str("\n\n");
    }
    result.push_str(&block);
    result.push('\n');
    result
}

/// Computes metrics for the canonical guidelines file if it exists.
pub fn canonical_guidelines_metrics() -> Result<Option<GuidelineMetrics>, String> {
    let canonical = canonical_guidelines_path()?;
    if canonical.exists() {
        let content = fs::read_to_string(&canonical)
            .map_err(|e| format!("Failed to read canonical guidelines: {e}"))?;
        Ok(Some(GuidelineMetrics::compute(&content)))
    } else {
        Ok(None)
    }
}

/// Safely updates the canonical guidelines file with an updated references block.
pub fn update_canonical_refs(refs_content: &str) -> Result<GuidelineMetrics, String> {
    let canonical = canonical_guidelines_path()?;
    let existing = if canonical.exists() {
        fs::read_to_string(&canonical)
            .map_err(|e| format!("Failed to read canonical guidelines: {e}"))?
    } else {
        if let Some(parent) = canonical.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {e}", parent.display()))?;
        }
        String::new()
    };

    let updated = inject_or_replace_refs_block(&existing, refs_content);
    crate::domain::storage::write_atomic_with_backup(&canonical, &updated)?;
    Ok(GuidelineMetrics::compute(&updated))
}

pub fn sync_guidelines() -> Result<Vec<GuidelineSyncResult>, String> {
    let canonical = canonical_guidelines_path()?;
    if !canonical.exists() {
        return Err(format!(
            "Canonical guidelines file not found at {}",
            canonical.display()
        ));
    }

    let targets = target_guidelines()?;
    let mut results = Vec::new();

    for target in targets {
        match create_link(&canonical, &target.path) {
            Ok(_) => results.push(GuidelineSyncResult {
                harness: target.harness,
                path: target.path,
                action: "linked".to_string(),
                success: true,
            }),
            Err(e) => results.push(GuidelineSyncResult {
                harness: target.harness,
                path: target.path,
                action: format!("failed: {e}"),
                success: false,
            }),
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_guidelines_count() {
        let targets = target_guidelines().expect("targets resolved");
        assert_eq!(targets.len(), 6);
    }

    #[test]
    fn test_canonical_path_ends_with_agents_md() {
        let canonical = canonical_guidelines_path().expect("canonical resolved");
        assert!(canonical.ends_with(".agents/AGENTS.md") || canonical.ends_with(".agents\\AGENTS.md"));
    }

    #[test]
    fn test_guideline_metrics_threshold() {
        let short_content = "# Short guidelines\nBe concise.\n";
        let metrics = GuidelineMetrics::compute(short_content);
        assert_eq!(metrics.line_count, 2);
        assert!(!metrics.exceeds_recommended_budget);

        let mut long_lines = String::new();
        for i in 0..305 {
            long_lines.push_str(&format!("Line {i}\n"));
        }
        let metrics_lines = GuidelineMetrics::compute(&long_lines);
        assert_eq!(metrics_lines.line_count, 305);
        assert!(metrics_lines.exceeds_recommended_budget);

        let long_chars = "x".repeat(15_001);
        let metrics_chars = GuidelineMetrics::compute(&long_chars);
        assert!(metrics_chars.exceeds_recommended_budget);
    }

    #[test]
    fn test_inject_or_replace_refs_block_fresh() {
        let initial = "# User Guidelines\n\nRule 1: Always test.";
        let refs = "- **pangolin**: `~/.local/share/maga/refs/pangolin`";

        let injected = inject_or_replace_refs_block(initial, refs);
        assert!(injected.contains(REFS_BLOCK_START));
        assert!(injected.contains(REFS_BLOCK_END));
        assert!(injected.contains("Rule 1: Always test."));
        assert!(injected.contains("- **pangolin**"));
    }

    #[test]
    fn test_inject_or_replace_refs_block_idempotent() {
        let initial = "# User Guidelines\n\nRule 1: Always test.";
        let refs1 = "- **v1**: path1";
        let injected1 = inject_or_replace_refs_block(initial, refs1);

        let refs2 = "- **v2**: path2";
        let injected2 = inject_or_replace_refs_block(&injected1, refs2);

        assert_eq!(injected2.matches(REFS_BLOCK_START).count(), 1);
        assert_eq!(injected2.matches(REFS_BLOCK_END).count(), 1);
        assert!(!injected2.contains("- **v1**"));
        assert!(injected2.contains("- **v2**"));
        assert!(injected2.contains("Rule 1: Always test."));
    }

    #[test]
    fn test_remove_refs_block_when_empty() {
        let initial = "# User Guidelines\n\n<!-- maga:refs:start -->\n- **v1**: path1\n<!-- maga:refs:end -->\n\nRule 2: Don't panic.";
        let stripped = inject_or_replace_refs_block(initial, "");
        assert!(!stripped.contains(REFS_BLOCK_START));
        assert!(!stripped.contains(REFS_BLOCK_END));
        assert!(stripped.contains("Rule 2: Don't panic."));
    }
}
