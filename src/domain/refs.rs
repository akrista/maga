use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for a single tracked documentation repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefRepoEntry {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Registry of reference documentation repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RefsRegistry {
    #[serde(default)]
    pub refs: BTreeMap<String, RefRepoEntry>,
}

impl RefsRegistry {
    pub fn from_toml(raw: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(raw)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Resolves the absolute path where the repository should be stored.
    pub fn effective_path(&self, alias: &str, entry: &RefRepoEntry, cache_root: &Path) -> PathBuf {
        if let Some(ref custom_path) = entry.path {
            if custom_path.starts_with("~/") || custom_path.starts_with("~\\") {
                if let Ok(home) = crate::domain::guidelines::home_dir() {
                    return home.join(&custom_path[2..]);
                }
            }
            PathBuf::from(custom_path)
        } else {
            cache_root.join(alias)
        }
    }
}

/// Clones or updates a reference repository using shallow git operations.
pub fn clone_or_update_ref(
    alias: &str,
    entry: &RefRepoEntry,
    target_dir: &Path,
) -> Result<String, String> {
    // Check if git is available
    let git_check = Command::new("git").arg("--version").output();
    if git_check.is_err() {
        return Err("`git` executable not found on system PATH.".to_string());
    }

    if target_dir.exists() && target_dir.join(".git").exists() {
        // Repository already exists, pull / fetch shallowly
        let mut cmd = Command::new("git");
        cmd.current_dir(target_dir);
        cmd.args(["pull", "--ff-only", "--depth", "1"]);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to update ref '{alias}': {e}"))?;

        if output.status.success() {
            Ok(format!("Updated ref '{alias}'"))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "Failed to update ref '{alias}': {}",
                stderr.trim()
            ))
        }
    } else {
        // Ensure parent directory exists
        if let Some(parent) = target_dir.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {e}", parent.display()))?;
        }

        let mut cmd = Command::new("git");
        cmd.args(["clone", "--depth", "1"]);

        if let Some(ref branch) = entry.branch {
            cmd.args(["--branch", branch]);
        }

        cmd.arg(&entry.url);
        cmd.arg(target_dir);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to clone ref '{alias}': {e}"))?;

        if output.status.success() {
            Ok(format!("Cloned ref '{alias}'"))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to clone ref '{alias}': {}", stderr.trim()))
        }
    }
}

/// Formats the compact markdown reference index block, including ONLY cloned repositories.
pub fn format_reference_index(registry: &RefsRegistry, cache_root: &Path) -> String {
    let cloned_refs: Vec<(&String, &RefRepoEntry, PathBuf)> = registry
        .refs
        .iter()
        .filter_map(|(alias, entry)| {
            let path = registry.effective_path(alias, entry, cache_root);
            if path.exists() && path.join(".git").exists() {
                Some((alias, entry, path))
            } else {
                None
            }
        })
        .collect();

    if cloned_refs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("### Offline Documentation References\n");
    out.push_str("When researching frameworks, libraries, or APIs listed below, ALWAYS inspect and read from these local paths before searching the web:\n\n");

    for (alias, entry, path) in cloned_refs {
        let path_str = path.display().to_string();
        if let Some(ref desc) = entry.description {
            out.push_str(&format!("- **{alias}** (`{path_str}`): {desc}\n"));
        } else {
            out.push_str(&format!("- **{alias}** (`{path_str}`)\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refs_registry_serde_roundtrip() {
        let toml_str = r#"
[refs.pangolin]
url = "https://github.com/fosrl/docs"
branch = "main"
description = "Pangolin documentation"

[refs.react]
url = "https://github.com/reactjs/react.dev"
description = "React docs"
"#;
        let registry: RefsRegistry = RefsRegistry::from_toml(toml_str).expect("deserialize toml");
        assert_eq!(registry.refs.len(), 2);
        assert_eq!(
            registry.refs.get("pangolin").unwrap().url,
            "https://github.com/fosrl/docs"
        );
        assert_eq!(
            registry.refs.get("pangolin").unwrap().branch.as_deref(),
            Some("main")
        );

        let serialized = registry.to_toml().expect("serialize toml");
        let parsed_again: RefsRegistry =
            RefsRegistry::from_toml(&serialized).expect("deserialize roundtrip");
        assert_eq!(registry, parsed_again);
    }

    #[test]
    fn test_format_reference_index() {
        let mut registry = RefsRegistry::default();
        registry.refs.insert(
            "pangolin".to_string(),
            RefRepoEntry {
                url: "https://github.com/fosrl/docs".to_string(),
                branch: Some("main".to_string()),
                description: Some("Pangolin documentation".to_string()),
                path: None,
            },
        );

        let temp = tempfile::tempdir().expect("tempdir");
        let cache_root = temp.path();

        // 1. Not cloned yet -> index must be empty
        let formatted_empty = format_reference_index(&registry, cache_root);
        assert!(formatted_empty.is_empty());

        // 2. Mock clone (create directory and .git folder)
        let repo_dir = cache_root.join("pangolin");
        fs::create_dir_all(repo_dir.join(".git")).expect("create mock .git");

        // 3. Now cloned -> index is formatted with offline instructions
        let formatted = format_reference_index(&registry, cache_root);
        assert!(formatted.contains("### Offline Documentation References"));
        assert!(formatted.contains("**pangolin**"));
        let expected_path = repo_dir.display().to_string();
        assert!(formatted.contains(&expected_path));
        assert!(formatted.contains("Pangolin documentation"));
    }

    #[test]
    fn test_clone_ref_invalid_url_returns_error() {
        let entry = RefRepoEntry {
            url: "https://127.0.0.1:9/invalid/no-such-repo-maga-test".to_string(),
            branch: None,
            description: None,
            path: None,
        };
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("invalid_repo");
        let result = clone_or_update_ref("invalid", &entry, &target);
        assert!(result.is_err());
    }
}
