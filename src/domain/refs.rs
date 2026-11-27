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
            if (custom_path.starts_with("~/") || custom_path.starts_with("~\\"))
                && let Ok(home) = crate::domain::guidelines::home_dir()
            {
                return home.join(&custom_path[2..]);
            }
            PathBuf::from(custom_path)
        } else {
            cache_root.join(alias)
        }
    }
}

fn run_git(dir: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git").current_dir(dir).args(args).output()
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
        // Follow the origin even if its history was rewritten (force-push, amend, orphan
        // branch): shallow fetch the tracked ref and hard-reset to it. `pull --ff-only`
        // would fail permanently once the remote history diverges from the clone.
        let remote_ref = entry.branch.as_deref().unwrap_or("HEAD");
        let fetch = run_git(target_dir, &["fetch", "--depth", "1", "origin", remote_ref])
            .map_err(|e| format!("Failed to update ref '{alias}': {e}"))?;
        if !fetch.status.success() {
            let stderr = String::from_utf8_lossy(&fetch.stderr);
            return Err(format!("Failed to update ref '{alias}': {}", stderr.trim()));
        }

        let reset = run_git(target_dir, &["reset", "--hard", "FETCH_HEAD"])
            .map_err(|e| format!("Failed to update ref '{alias}': {e}"))?;
        if reset.status.success() {
            Ok(format!("Updated ref '{alias}'"))
        } else {
            let stderr = String::from_utf8_lossy(&reset.stderr);
            Err(format!("Failed to update ref '{alias}': {}", stderr.trim()))
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

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=maga-test",
                "-c",
                "user.email=maga-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn file_url(path: &Path) -> String {
        let p = path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        format!("file:///{}", p.trim_start_matches('/'))
    }

    #[test]
    fn test_update_ref_follows_rewritten_remote_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let origin = temp.path().join("origin");
        fs::create_dir_all(&origin).expect("origin dir");
        git_ok(&origin, &["init", "-b", "main"]);
        fs::write(origin.join("doc.md"), "v1").expect("write v1");
        git_ok(&origin, &["add", "."]);
        git_ok(&origin, &["commit", "-m", "v1"]);

        let entry = RefRepoEntry {
            url: file_url(&origin),
            branch: Some("main".to_string()),
            description: None,
            path: None,
        };
        let target = temp.path().join("clone");
        clone_or_update_ref("docs", &entry, &target).expect("initial clone");
        assert_eq!(fs::read_to_string(target.join("doc.md")).unwrap(), "v1");

        // Rewrite origin history: the new tip is unrelated to the cloned commit.
        git_ok(&origin, &["checkout", "--orphan", "rewritten"]);
        fs::write(origin.join("doc.md"), "v2").expect("write v2");
        git_ok(&origin, &["add", "."]);
        git_ok(&origin, &["commit", "-m", "rewritten history"]);
        git_ok(&origin, &["branch", "-M", "main"]);

        let msg = clone_or_update_ref("docs", &entry, &target).expect("update after rewrite");
        assert_eq!(msg, "Updated ref 'docs'");
        assert_eq!(fs::read_to_string(target.join("doc.md")).unwrap(), "v2");
    }
}
