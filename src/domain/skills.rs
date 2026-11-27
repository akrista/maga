//! Thin wrapper around the `skills` CLI (<https://skills.sh>): search, install and per-harness
//! activation. maga never touches `~/.agents/skills` or the harness skill dirs itself; the
//! CLI owns the canonical copy and the per-agent links.

use crate::domain::model::HarnessKind;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// Where a skill is installed: user-level (`-g`) or in the current project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Global,
    Project,
}

/// `skills` CLI agent id (`-a <id>`) for a harness.
pub fn agent_id(kind: HarnessKind) -> &'static str {
    match kind {
        HarnessKind::Antigravity => "antigravity",
        HarnessKind::Claude => "claude-code",
        HarnessKind::Copilot => "github-copilot",
        HarnessKind::Grok => "grok",
        HarnessKind::OpenCode => "opencode",
    }
}

/// Agent display name as printed in `skills ls --json`.
fn agent_label(kind: HarnessKind) -> &'static str {
    match kind {
        HarnessKind::Antigravity => "Antigravity",
        HarnessKind::Claude => "Claude Code",
        HarnessKind::Copilot => "GitHub Copilot",
        HarnessKind::Grok => "Grok Build",
        HarnessKind::OpenCode => "OpenCode",
    }
}

/// One entry of `skills ls --json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstalledSkill {
    pub name: String,
    pub path: String,
    /// `"global"` or `"project"`.
    pub scope: String,
    #[serde(default)]
    pub agents: Vec<String>,
    /// Origin recorded by the CLI (`owner/repo`); `None` for hand-made skills.
    #[serde(default)]
    pub source: Option<String>,
}

impl InstalledSkill {
    pub fn scope(&self) -> SkillScope {
        if self.scope == "project" {
            SkillScope::Project
        } else {
            SkillScope::Global
        }
    }

    pub fn enabled_for(&self, kind: HarnessKind) -> bool {
        self.agents.iter().any(|a| a == agent_label(kind))
    }
}

/// One result of `skills find`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// `owner/repo@skill`, directly accepted by `skills add`.
    pub package: String,
    pub installs: String,
    pub url: String,
}

fn scope_args(scope: SkillScope) -> Option<&'static str> {
    (scope == SkillScope::Global).then_some("-g")
}

/// Arguments to install `package` for the given harnesses.
pub fn install_args(package: &str, scope: SkillScope, harnesses: &[HarnessKind]) -> Vec<String> {
    let mut args = vec!["add".to_string(), package.to_string(), "-y".to_string()];
    args.extend(scope_args(scope).map(String::from));
    for kind in harnesses {
        args.push("-a".to_string());
        args.push(agent_id(*kind).to_string());
    }
    args
}

/// Arguments to link/unlink one installed skill for one harness. Linking needs the recorded
/// source, because the CLI can only (re)link from a source.
pub fn set_enabled_args(
    skill: &InstalledSkill,
    kind: HarnessKind,
    enable: bool,
) -> Result<Vec<String>, String> {
    let scope = scope_args(skill.scope());
    let mut args: Vec<String> = if enable {
        let source = skill.source.as_deref().ok_or_else(|| {
            format!(
                "'{}' has no recorded source; reinstall it from search (f) to manage harnesses",
                skill.name
            )
        })?;
        vec!["add".into(), source.into()]
    } else {
        vec!["remove".into()]
    };
    args.extend(["-s".into(), skill.name.clone(), "-y".into()]);
    args.extend(scope.map(String::from));
    args.extend(["-a".into(), agent_id(kind).into()]);
    Ok(args)
}

/// Removes ANSI escape sequences (the CLI colors output even when piped on some platforms).
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses `skills find` output: `owner/repo@skill 1.2K installs` followed by `└ <url>`.
pub fn parse_search_output(raw: &str) -> Vec<SearchHit> {
    let text = strip_ansi(raw);
    let mut hits: Vec<SearchHit> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(url) = line.strip_prefix('└') {
            if let Some(last) = hits.last_mut()
                && last.url.is_empty()
            {
                last.url = url.trim().to_string();
            }
            continue;
        }
        let mut words = line.split_whitespace();
        if let (Some(package), Some(count), Some("installs"), None) =
            (words.next(), words.next(), words.next(), words.next())
            && package.contains('@')
        {
            hits.push(SearchHit {
                package: package.to_string(),
                installs: count.to_string(),
                url: String::new(),
            });
        }
    }
    hits
}

/// Parses `skills ls --json`, skipping any banner the CLI prints before the JSON array.
pub fn parse_installed_output(raw: &str) -> Result<Vec<InstalledSkill>, String> {
    let text = strip_ansi(raw);
    let start = text
        .lines()
        .scan(0usize, |offset, line| {
            let here = *offset;
            *offset += line.len() + 1;
            Some((here, line.trim()))
        })
        .find(|(_, line)| *line == "[" || line.starts_with("[]") || line.starts_with("[{"))
        .map(|(offset, _)| offset)
        .ok_or_else(|| "skills ls produced no JSON output".to_string())?;
    serde_json::from_str(&text[start..]).map_err(|e| format!("Invalid skills ls output: {e}"))
}

/// Runs the `skills` CLI in `cwd` and returns its stdout.
///
/// Launcher: `MAGA_SKILLS_CMD` (whitespace-separated, e.g. `pnpm dlx skills`), else
/// `bunx skills`, else `npx -y skills`.
pub fn run_skills(args: &[String], cwd: &Path) -> Result<String, String> {
    let launcher: Vec<String> = match std::env::var("MAGA_SKILLS_CMD") {
        Ok(custom) if !custom.trim().is_empty() => {
            custom.split_whitespace().map(String::from).collect()
        }
        _ if Command::new("bunx").arg("--version").output().is_ok() => {
            vec!["bunx".into(), "skills".into()]
        }
        _ => vec![
            if cfg!(windows) { "npx.cmd" } else { "npx" }.into(),
            "-y".into(),
            "skills".into(),
        ],
    };

    let output = Command::new(&launcher[0])
        .args(&launcher[1..])
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Could not run `{}`: {e} (install bun or node)", launcher[0]))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = strip_ansi(&format!("{stdout}\n{stderr}"))
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || "│■╰▶".contains(c)))
        .rfind(|l| !l.is_empty())
        .unwrap_or("unknown error")
        .to_string();
    Err(format!(
        "skills {}: {detail}",
        args.first().map_or("", |s| s)
    ))
}

/// Installed skills: global ones plus those of the project in `cwd`.
pub fn list_installed(cwd: &Path) -> Result<Vec<InstalledSkill>, String> {
    let mut all = parse_installed_output(&run_skills(
        &["ls".into(), "--json".into(), "-g".into()],
        cwd,
    )?)?;
    all.extend(parse_installed_output(&run_skills(
        &["ls".into(), "--json".into()],
        cwd,
    )?)?);
    all.sort_by(|a, b| (a.scope != "global", &a.name).cmp(&(b.scope != "global", &b.name)));
    all.dedup_by(|a, b| a.scope == b.scope && a.name == b.name);
    Ok(all)
}

/// Searches skills.sh through `skills find <query>`.
pub fn search(query: &str, cwd: &Path) -> Result<Vec<SearchHit>, String> {
    run_skills(&["find".into(), query.into()], cwd).map(|out| parse_search_output(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(source: Option<&str>, scope: &str) -> InstalledSkill {
        InstalledSkill {
            name: "tui-design".to_string(),
            path: "x".to_string(),
            scope: scope.to_string(),
            agents: vec!["Claude Code".to_string()],
            source: source.map(String::from),
        }
    }

    #[test]
    fn parses_find_output_with_ansi_and_urls() {
        let raw = "\u{1b}[2mInstall with npx skills add <owner/repo@skill>\u{1b}[0m\n\n\
            msmps/opentui-skill@opentui 64.7K installs\n└ https://skills.sh/msmps/opentui-skill/opentui\n\n\
            hyperb1iss/hyperskills@tui-design 1.4K installs\n└ https://skills.sh/h/h/tui-design\n";
        let hits = parse_search_output(raw);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].package, "msmps/opentui-skill@opentui");
        assert_eq!(hits[0].installs, "64.7K");
        assert_eq!(hits[1].url, "https://skills.sh/h/h/tui-design");
    }

    #[test]
    fn parses_ls_json_after_banner_and_maps_agents_to_harnesses() {
        let raw = "\u{1b}[?25l│ banner [x]\n[\n  {\"name\":\"a\",\"path\":\"p\",\"scope\":\"global\",\
            \"agents\":[\"Claude Code\",\"Grok Build\"],\"source\":null}\n]\n";
        let skills = parse_installed_output(raw).expect("parse");
        assert_eq!(skills.len(), 1);
        assert!(skills[0].enabled_for(HarnessKind::Claude));
        assert!(skills[0].enabled_for(HarnessKind::Grok));
        assert!(!skills[0].enabled_for(HarnessKind::OpenCode));
        assert!(parse_installed_output("no json here").is_err());
        assert!(parse_installed_output("[]").expect("empty").is_empty());
    }

    #[test]
    fn install_args_cover_scope_and_agents() {
        assert_eq!(
            install_args(
                "o/r@s",
                SkillScope::Global,
                &[HarnessKind::Claude, HarnessKind::Antigravity]
            ),
            [
                "add",
                "o/r@s",
                "-y",
                "-g",
                "-a",
                "claude-code",
                "-a",
                "antigravity"
            ]
        );
        assert_eq!(
            install_args("o/r@s", SkillScope::Project, &[HarnessKind::OpenCode]),
            ["add", "o/r@s", "-y", "-a", "opencode"]
        );
    }

    #[test]
    fn set_enabled_args_unlink_relink_and_missing_source() {
        let tracked = skill(Some("pageton/tui-design-skill"), "global");
        assert_eq!(
            set_enabled_args(&tracked, HarnessKind::Claude, false).unwrap(),
            [
                "remove",
                "-s",
                "tui-design",
                "-y",
                "-g",
                "-a",
                "claude-code"
            ]
        );
        assert_eq!(
            set_enabled_args(&tracked, HarnessKind::Grok, true).unwrap(),
            [
                "add",
                "pageton/tui-design-skill",
                "-s",
                "tui-design",
                "-y",
                "-g",
                "-a",
                "grok"
            ]
        );
        let project = skill(Some("o/r"), "project");
        assert!(
            !set_enabled_args(&project, HarnessKind::Claude, true)
                .unwrap()
                .contains(&"-g".to_string())
        );
        let untracked = skill(None, "global");
        assert!(set_enabled_args(&untracked, HarnessKind::Claude, true).is_err());
        assert!(set_enabled_args(&untracked, HarnessKind::Claude, false).is_ok());
    }
}
