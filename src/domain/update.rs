use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::io::{Read, Write};
use std::time::Duration;

pub const DEFAULT_REPO: &str = "akrista/maga";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl SemVer {
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim().trim_start_matches('v').trim_start_matches('V');
        let parts: Vec<&str> = clean.splitn(2, '-').collect();
        let core = parts[0];
        let pre = parts.get(1).map(|p| p.to_string());

        let nums: Vec<&str> = core.split('.').collect();
        if nums.len() != 3 {
            return None;
        }
        let major = nums[0].parse().ok()?;
        let minor = nums[1].parse().ok()?;
        let patch = nums[2].parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(b),
            },
            ord => ord,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub is_newer: bool,
    pub download_url: Option<String>,
    pub checksum_url: Option<String>,
    pub release_notes_url: Option<String>,
    pub target: String,
    pub is_zip: bool,
}

pub fn current_target() -> Option<(&'static str, &'static str)> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some(("x86_64-pc-windows-msvc", "zip"))
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(("x86_64-unknown-linux-gnu", "tar.gz"))
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Some(("aarch64-unknown-linux-gnu", "tar.gz"))
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Some(("x86_64-apple-darwin", "tar.gz"))
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some(("aarch64-apple-darwin", "tar.gz"))
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "macos", any(target_arch = "x86_64", target_arch = "aarch64"))
    )))]
    {
        None
    }
}

pub fn check_latest_release(repo: &str) -> Result<UpdateInfo, String> {
    let (target, ext) = current_target().ok_or_else(|| {
        "Unsupported host platform/architecture for binary self-update".to_string()
    })?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();

    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let mut resp = agent
        .get(&url)
        .header("User-Agent", &format!("maga/{CURRENT_VERSION}"))
        .header("Accept", "application/vnd.github.v3+json")
        .call()
        .map_err(|e| format!("Failed to reach GitHub Releases API: {e}"))?;

    let status = resp.status().as_u16();
    if status >= 400 {
        return Err(format!("GitHub API returned HTTP {status}"));
    }

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read GitHub response: {e}"))?;

    let val: Value = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse GitHub release JSON: {e}"))?;

    let tag_name = val
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing tag_name in release payload".to_string())?;

    let release_notes_url = val
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let remote_semver = SemVer::parse(tag_name)
        .ok_or_else(|| format!("Could not parse remote version tag: {tag_name}"))?;
    let local_semver = SemVer::parse(CURRENT_VERSION)
        .ok_or_else(|| format!("Could not parse local version: {CURRENT_VERSION}"))?;

    let is_newer = remote_semver > local_semver;

    let expected_asset_suffix = format!("{target}.{ext}");
    let expected_sha_suffix = format!("{target}.{ext}.sha256");

    let mut download_url = None;
    let mut checksum_url = None;

    if let Some(assets) = val.get("assets").and_then(|v| v.as_array()) {
        for asset in assets {
            if let Some(name) = asset.get("name").and_then(|v| v.as_str()) {
                if name.ends_with(&expected_sha_suffix) {
                    checksum_url = asset
                        .get("browser_download_url")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                } else if name.ends_with(&expected_asset_suffix) {
                    download_url = asset
                        .get("browser_download_url")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                }
            }
        }
    }

    Ok(UpdateInfo {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: tag_name.to_string(),
        is_newer,
        download_url,
        checksum_url,
        release_notes_url,
        target: target.to_string(),
        is_zip: ext == "zip",
    })
}

pub fn download_and_verify(
    download_url: &str,
    checksum_url: Option<&str>,
) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into();

    let mut resp = agent
        .get(download_url)
        .header("User-Agent", &format!("maga/{CURRENT_VERSION}"))
        .call()
        .map_err(|e| format!("Failed to download asset: {e}"))?;

    let status = resp.status().as_u16();
    if status >= 400 {
        return Err(format!("Download failed with HTTP {status}"));
    }

    let mut archive_bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .read_to_end(&mut archive_bytes)
        .map_err(|e| format!("Failed to read asset data: {e}"))?;

    if let Some(sha_url) = checksum_url {
        let mut sha_resp = agent
            .get(sha_url)
            .header("User-Agent", &format!("maga/{CURRENT_VERSION}"))
            .call()
            .map_err(|e| format!("Failed to download checksum: {e}"))?;

        if sha_resp.status().as_u16() < 400 {
            let sha_text = sha_resp
                .body_mut()
                .read_to_string()
                .map_err(|e| format!("Failed to read checksum data: {e}"))?;
            let expected_sha = sha_text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim()
                .to_lowercase();

            if !expected_sha.is_empty() {
                let mut hasher = Sha256::new();
                hasher.update(&archive_bytes);
                let computed_sha = format!("{:x}", hasher.finalize());

                if computed_sha != expected_sha {
                    return Err(format!(
                        "Checksum verification failed! Expected {expected_sha}, computed {computed_sha}"
                    ));
                }
            }
        }
    }

    Ok(archive_bytes)
}

pub fn extract_binary(
    archive_bytes: &[u8],
    is_zip: bool,
) -> Result<tempfile::NamedTempFile, String> {
    let mut temp = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temporary file for update: {e}"))?;

    if is_zip {
        let reader = std::io::Cursor::new(archive_bytes);
        let mut zip = zip::ZipArchive::new(reader)
            .map_err(|e| format!("Failed to parse zip archive: {e}"))?;

        let mut found = false;
        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry #{i}: {e}"))?;
            let name = file.name().to_string();
            let base_name = std::path::Path::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if base_name == "maga.exe" || base_name == "maga" {
                std::io::copy(&mut file, &mut temp)
                    .map_err(|e| format!("Failed extracting {base_name}: {e}"))?;
                found = true;
                break;
            }
        }
        if !found {
            return Err("Binary 'maga' or 'maga.exe' not found inside zip archive".to_string());
        }
    } else {
        let gz = flate2::read::GzDecoder::new(archive_bytes);
        let mut tar = tar::Archive::new(gz);

        let mut found = false;
        let entries = tar
            .entries()
            .map_err(|e| format!("Failed to read tar entries: {e}"))?;

        for entry in entries {
            let mut entry = entry.map_err(|e| format!("Corrupted tar entry: {e}"))?;
            let path = entry
                .path()
                .map_err(|e| format!("Invalid path in tar: {e}"))?
                .to_path_buf();
            let base_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

            if base_name == "maga" || base_name == "maga.exe" {
                std::io::copy(&mut entry, &mut temp)
                    .map_err(|e| format!("Failed extracting {base_name}: {e}"))?;
                found = true;
                break;
            }
        }
        if !found {
            return Err("Binary 'maga' not found inside tar archive".to_string());
        }
    }

    temp.flush()
        .map_err(|e| format!("Failed flushing temp file: {e}"))?;
    Ok(temp)
}

pub fn apply_update(info: &UpdateInfo) -> Result<(), String> {
    let download_url = info.download_url.as_deref().ok_or_else(|| {
        format!(
            "No download asset found for platform target '{}'",
            info.target
        )
    })?;

    let archive_bytes = download_and_verify(download_url, info.checksum_url.as_deref())?;
    let temp_binary = extract_binary(&archive_bytes, info.is_zip)?;

    self_replace::self_replace(temp_binary.path())
        .map_err(|e| format!("Failed to replace current binary: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parsing() {
        let v = SemVer::parse("v0.3.0").unwrap();
        assert_eq!(
            v,
            SemVer {
                major: 0,
                minor: 3,
                patch: 0,
                pre: None
            }
        );

        let v2 = SemVer::parse("1.2.3-beta.1").unwrap();
        assert_eq!(
            v2,
            SemVer {
                major: 1,
                minor: 2,
                patch: 3,
                pre: Some("beta.1".to_string())
            }
        );

        assert!(SemVer::parse("invalid").is_none());
    }

    #[test]
    fn test_semver_ordering() {
        let v0_3_0 = SemVer::parse("v0.3.0").unwrap();
        let v0_3_1 = SemVer::parse("v0.3.1").unwrap();
        let v0_4_0 = SemVer::parse("0.4.0").unwrap();
        let v1_0_0 = SemVer::parse("v1.0.0").unwrap();
        let v1_0_0_beta = SemVer::parse("1.0.0-beta").unwrap();

        assert!(v0_3_1 > v0_3_0);
        assert!(v0_4_0 > v0_3_1);
        assert!(v1_0_0 > v0_4_0);
        assert!(v1_0_0 > v1_0_0_beta);
    }

    #[test]
    fn test_current_target_validity() {
        let target = current_target();
        assert!(target.is_some(), "Current host target should be supported");
        let (triple, ext) = target.unwrap();
        assert!(!triple.is_empty());
        assert!(ext == "zip" || ext == "tar.gz");
    }
}
