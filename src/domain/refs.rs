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
            // `fetch origin HEAD` leaves `origin/<branch>` stale, which makes git report a bogus
            // ahead/behind. Point it at what was just fetched (best effort).
            if let Ok(branch) = run_git(target_dir, &["symbolic-ref", "--short", "-q", "HEAD"])
                && branch.status.success()
            {
                let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
                if !branch.is_empty() {
                    let _ = run_git(
                        target_dir,
                        &[
                            "update-ref",
                            &format!("refs/remotes/origin/{branch}"),
                            "FETCH_HEAD",
                        ],
                    );
                }
            }
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

/// What the Refs inspector shows about a local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    /// Attached branch, `None` when HEAD is detached (tag or commit).
    pub branch: Option<String>,
    pub commit: String,
    /// Commit date, `YYYY-MM-DD`.
    pub date: String,
    pub subject: String,
    /// Modified, deleted or untracked paths (`git status --porcelain`).
    pub dirty: usize,
    pub shallow: bool,
}

/// Reads the state of the clone in `dir`; `None` when it is not a usable git checkout.
pub fn repo_info(dir: &Path) -> Option<RepoInfo> {
    if !dir.join(".git").exists() {
        return None;
    }
    let git = |args: &[&str]| -> Option<String> {
        run_git(dir, args)
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let log = git(&["log", "-1", "--format=%h%x09%cs%x09%s"])?;
    let mut parts = log.splitn(3, '\t');
    let commit = parts.next()?.to_string();
    let date = parts.next()?.to_string();
    let subject = parts.next().unwrap_or_default().to_string();
    Some(RepoInfo {
        branch: git(&["symbolic-ref", "--short", "-q", "HEAD"]).filter(|b| !b.is_empty()),
        commit,
        date,
        subject,
        dirty: git(&["status", "--porcelain"]).map_or(0, |s| s.lines().count()),
        shallow: git(&["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true"),
    })
}

/// Test helpers shared with other modules' tests.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;
    use std::process::Command;

    pub fn git_ok(dir: &Path, args: &[&str]) {
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

    pub fn file_url(path: &Path) -> String {
        let p = path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        format!("file:///{}", p.trim_start_matches('/'))
    }

    /// A one-commit repository on branch `main` with `doc.md` containing `content`.
    pub fn make_origin(dir: &Path, content: &str) {
        std::fs::create_dir_all(dir).expect("origin dir");
        git_ok(dir, &["init", "-b", "main"]);
        std::fs::write(dir.join("doc.md"), content).expect("write doc");
        git_ok(dir, &["add", "."]);
        git_ok(dir, &["commit", "-m", content]);
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{file_url, git_ok, make_origin};
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

        // origin/main follows the update, so git does not report a bogus ahead/behind.
        let counts = run_git(
            &target,
            &["rev-list", "--left-right", "--count", "HEAD...origin/main"],
        )
        .expect("rev-list");
        assert_eq!(String::from_utf8_lossy(&counts.stdout).trim(), "0	0");
    }

    #[test]
    fn repo_info_reports_branch_commit_dirty_and_shallow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let origin = temp.path().join("origin");
        make_origin(&origin, "first doc");

        let target = temp.path().join("clone");
        let entry = RefRepoEntry {
            url: file_url(&origin),
            branch: None,
            description: None,
            path: None,
        };
        assert!(repo_info(&target).is_none(), "not cloned yet");
        clone_or_update_ref("docs", &entry, &target).expect("clone");

        let info = repo_info(&target).expect("info");
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.subject, "first doc");
        assert_eq!(info.commit.len(), info.commit.trim().len());
        assert_eq!(info.date.len(), 10, "YYYY-MM-DD: {}", info.date);
        assert_eq!(info.dirty, 0);
        assert!(info.shallow, "maga clones with --depth 1");

        fs::write(target.join("doc.md"), "edited").expect("edit");
        fs::write(target.join("new.md"), "new").expect("add");
        assert_eq!(repo_info(&target).expect("info").dirty, 2);

        // Updating discards the edits, as documented.
        clone_or_update_ref("docs", &entry, &target).expect("update");
        assert_eq!(
            repo_info(&target).expect("info").dirty,
            1,
            "untracked file stays"
        );
    }
}
