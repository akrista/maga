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
    /// The canonical file (`~/.agents/AGENTS.md`) exists; without it nothing can be compared.
    pub canonical_present: bool,
}

/// What a harness guidelines file's state means for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidelineHealth {
    InSync,
    /// Present, but its content differs from the canonical file.
    Drift,
    /// Present, but the canonical file is missing, so it cannot be verified (not real drift).
    CanonicalMissing,
    Missing,
}

impl GuidelineStatus {
    pub fn health(&self) -> GuidelineHealth {
        if !self.exists {
            GuidelineHealth::Missing
        } else if !self.canonical_present {
            GuidelineHealth::CanonicalMissing
        } else if self.matches_canonical {
            GuidelineHealth::InSync
        } else {
            GuidelineHealth::Drift
        }
    }
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
            canonical_present: canonical_bytes.is_some(),
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
        let exceeds_recommended_budget =
            line_count > DEFAULT_MAX_GUIDELINE_LINES || char_count > DEFAULT_MAX_GUIDELINE_CHARS;

        Self {
            line_count,
            char_count,
            exceeds_recommended_budget,
        }
    }
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
        assert!(
            canonical.ends_with(".agents/AGENTS.md") || canonical.ends_with(".agents\\AGENTS.md")
        );
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
    fn health_separates_a_missing_canonical_from_real_drift() {
        let status = |exists, canonical_present, matches_canonical| GuidelineStatus {
            harness: target_guidelines().expect("targets")[0].harness,
            path: PathBuf::from("x"),
            exists,
            is_symlink: true,
            matches_canonical,
            canonical_present,
        };
        assert_eq!(status(true, true, true).health(), GuidelineHealth::InSync);
        assert_eq!(status(true, true, false).health(), GuidelineHealth::Drift);
        assert_eq!(
            status(true, false, false).health(),
            GuidelineHealth::CanonicalMissing,
            "no canonical file: unverifiable, not drift"
        );
        assert_eq!(
            status(false, true, false).health(),
            GuidelineHealth::Missing
        );
        assert_eq!(
            status(false, false, false).health(),
            GuidelineHealth::Missing
        );
    }
}
