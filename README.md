<div align="center">
  <h1>✦ maga ✦</h1>
  <p><strong>Unified Terminal UI &amp; Canonical Registry for Model Context Protocol (MCP) Servers &amp; Agentic Harness Plugins</strong></p>

  <p>
    <a href="#quick-start"><img src="https://img.shields.io/badge/version-0.3.0-e8998d?style=flat-square" alt="Version 0.3.0" /></a>
    <a href="Cargo.toml"><img src="https://img.shields.io/badge/rust-2024_edition-f4b942?style=flat-square&logo=rust" alt="Rust 2024 Edition" /></a>
    <a href="#supported-harnesses"><img src="https://img.shields.io/badge/harnesses-5_supported-7b68ee?style=flat-square" alt="5 Supported Harnesses" /></a>
    <a href="#live-mcp-health-probing--plugin-management"><img src="https://img.shields.io/badge/workspace-MCPs_%2B_Plugins-4ecdc4?style=flat-square" alt="MCPs + Plugins" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-6cc644?style=flat-square" alt="MIT License" /></a>
  </p>

  <p>
    <a href="#features">Features</a> ·
    <a href="#supported-harnesses">Harnesses</a> ·
    <a href="#live-mcp-health-probing--plugin-management">Probing &amp; Plugins</a> ·
    <a href="#quick-start">Quick Start</a> ·
    <a href="#keyboard-shortcuts">Shortcuts</a> ·
    <a href="#live-filtering--search-syntax">Filtering</a> ·
    <a href="#configuration-files">Configuration</a> ·
    <a href="#3-tier-spellcaster-mascot--terminal-compatibility">Mascot &amp; Braille Engine</a>
  </p>

  <br />

  <p><em>Manage, probe, reconcile, and synchronize your global AI agent ecosystem from one arcane terminal workspace.</em></p>
</div>

---

## Features

**`maga`** reconciles, probes, edits, toggles, and synchronizes your global **MCP servers** and **harness plugins** across **Antigravity**, **Claude Code**, **GitHub Copilot**, **Grok CLI**, and **OpenCode** from a single keyboard-first terminal workspace.

```text
         ┌──────────────────────────────────────────────────────────────┐
         │                     ~/.config/maga/                          │
         │  mcp.toml (Canonical MCPs) · plugins.toml · config.toml (UI) │
         └──────────────────────────────┬───────────────────────────────┘
                                        │
               ┌────────────────────────┼────────────────────────┐
               ▼                        ▼                        ▼
      ┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
      │   Antigravity   │      │   Claude Code   │      │ GitHub Copilot  │
      │ mcp_config.json │      │  ~/.claude.json │      │ mcp-config.json │
      │ + config.json   │      │ + settings.json │      │ + config.json   │
      └─────────────────┘      └─────────────────┘      └─────────────────┘
               ▲                                                 ▲
               └──────────────┐                   ┌──────────────┘
                              ▼                   ▼
                     ┌─────────────────┐ ┌─────────────────┐
                     │    Grok CLI     │ │    OpenCode     │
                     │   config.toml   │ │  opencode.json  │
                     │  (MCPs+Plugins) │ │  (MCPs+Plugins) │
                     └─────────────────┘ └─────────────────┘
```

- **Canonical MCP Registry (`~/.config/maga/mcp.toml`)**: Define `stdio` (`command`, `args`, `env`, `description`) and `http` (`url`, `headers`, `description`) MCP servers once, cleanly separated from UI/tool preferences in `~/.config/maga/config.toml` and plugin state in `~/.config/maga/plugins.toml`.
- **Two-Way Reconciliation & Conflict Detection**: Automatically imports untracked servers on startup and live reload, detects definition drift or case-insensitive name collisions (e.g., `"Context7"` vs `"context7"`), and groups identical definitions into numbered variants (`[1]`..`[N]`) for side-by-side resolution.
- **Per-Harness Plugin Management (`Tab` / `1` / `2`)**: Discovers installed and configured plugins across all 5 harnesses, extracts bundled `skills`, `hooks`, and `MCPs` counts/identifiers, toggles plugins in each harness's native config file, retains disabled OpenCode npm specifiers in `plugins.toml`, and supports complete native uninstall (`d`).
- **Live MCP Health Probing & Tool Inspection (`p` / `P` / `t`)**: Executes non-blocking, bounded-timeout (8s) JSON-RPC `initialize` + `tools/list` handshakes over `stdio` subprocesses and `http` / SSE endpoints, displaying live round-trip latency (`ms`) and exposed tool definitions.
- **Lossless Schema Translation & Symlink-Safe Atomic Backups**: Writes `.bak` backups before every update via canonicalized same-directory atomic temp-file replacement—preserving symbolic links, non-MCP keys, OAuth/session state, `"plugin:*"` entries, and per-server harness options.
- **Hybrid Editing & Live Auto-Refresh**: Add or edit servers via a guided in-TUI form (`a` / `e`) or directly as validated TOML in `$EDITOR` (`E`), while background disk polling reconciles external edits in real time.
- **In-Place Self-Updates & Status Bar Alerts (`--update`)**: Non-blocking background release checks against GitHub Releases with an unobtrusive status bar alert, and verified in-place SHA-256 binary upgrades via `maga --update`.
- **3-Tier Reactive Spellcaster UI**: Unicode Braille (`⠁..⣿`, `2×4` sub-cell dot matrix) artwork, keyboard-focus gaze tracking, action-reactive animations (`Idle`, `Casting`, `Scrying`, `Startled`, `Refreshing`), `tachyonfx` shader transitions, and three semantic palettes (`blush`, `indigo`, `elite`).

---

## Supported Harnesses

| Harness | Config Key | Default MCP Config Path | Native Plugin Config & Discovery Paths | Preserved Harness Metadata |
| :--- | :--- | :--- | :--- | :--- |
| **Antigravity** | `antigravity` | `~/.gemini/config/mcp_config.json` (`"mcpServers"`) | `~/.gemini/config/config.json` (`"plugins"`), `~/.gemini/config/plugins/*` | Legacy `"serverUrl"` normalization, `"disabled": true` state, `"userSettings"` |
| **Claude Code** | `claude` | `~/.claude.json` (`"mcpServers"`, `"projects"`) | `~/.claude/settings.json` (`"enabledPlugins"`), `~/.claude/plugins/installed_plugins.json` | Root & per-env `"enabledMcpServers"` / `"disabledMcpServers"`, `"plugin:*"` entries, `"hooks"`, `"permissions"`, OAuth & project state |
| **GitHub Copilot** | `copilot` | `~/.copilot/mcp-config.json` (`"mcpServers"`) | `~/.copilot/config.json` (`"installedPlugins"`), `~/.copilot/installed-plugins/*` | `"local"` / `"sse"` transport normalization, per-server `"tools"` (defaults to `["*"]`) & `"timeout"`, `"trustedFolders"`, `"loggedInUsers"` |
| **Grok CLI** | `grok` | `~/.grok/config.toml` (`[mcp_servers.<name>]`) | `~/.grok/config.toml` (`[plugins].paths` / `disabled`), `~/.grok/installed-plugins/*` | Non-MCP tables (`[cli]`, `[marketplace]`, `[models]`, `[ui]`), `startup_timeout_sec`, `tool_timeout_sec`, `tool_timeouts` |
| **OpenCode** | `opencode` | `~/.config/opencode/opencode.json` (`"mcp"`) | `~/.config/opencode/opencode.json` (`"plugin"` / `"plugins"`), `~/.config/opencode/plugins/*`, `plugins.toml` | `"local"` (`[command, ...args]`, `"environment"`) & `"remote"` (`"url"`, `"headers"`), `"$schema"`, `"instructions"`, `"permission"`, `"provider"`, `"oauth"`, `"cwd"` |

---

## Live MCP Health Probing & Plugin Management

### 1. Live MCP Health Probing & Tool Discovery (<kbd>p</kbd> / <kbd>P</kbd> / <kbd>t</kbd>)

Verify server reachability and inspect exposed tools directly from the `MCPs` workspace without blocking the UI:

- **Protocol Handshake (`initialize` + `tools/list`)**: Press <kbd>p</kbd> to probe the selected MCP server or <kbd>P</kbd> to probe all servers concurrently on background worker threads within an **8-second bounded timeout**.
- **Dual-Transport Support**:
  - **`stdio`**: Spawns the configured `command` with `args` and `env`, exchanges JSON-RPC `initialize`, `notifications/initialized`, and `tools/list` frames over `stdin`/`stdout`, and cleanly terminates and reaps the subprocess upon completion or timeout.
  - **`http`**: Sends JSON-RPC POST requests with all configured `headers`, propagates any `Mcp-Session-Id` session header across requests, and parses both `application/json` and `text/event-stream` (`SSE` `data:` frames).
- **Latency & Interactive Tool Inspection**: Displays online/offline status, server implementation name/version, round-trip latency (`ms`), and exposed tool definitions (`name` + `description`). Press <kbd>t</kbd> to focus and scroll the `DETAILS` tool list when a server exposes more tools than fit on screen.

### 2. Per-Harness Plugin Management (<kbd>Tab</kbd> / <kbd>1</kbd> / <kbd>2</kbd>)

Switch between the **MCP Servers (`1`)** and **Plugins (`2`)** workspace sections at any time using <kbd>Tab</kbd>, <kbd>Shift+Tab</kbd>, <kbd>1</kbd>, or <kbd>2</kbd>:

- **Hierarchical Harness Tree & Folding**: Plugins are grouped by enabled harness (**Antigravity**, **Claude Code**, **GitHub Copilot**, **Grok CLI**, **OpenCode**). Use <kbd>Enter</kbd> on a harness header to toggle folding, or <kbd>h</kbd> / <kbd>←</kbd> and <kbd>l</kbd> / <kbd>→</kbd> to collapse/expand harness groups.
- **Bundled Customization Inspection**: Automatically inspects each plugin's `install_path` manifest (`plugin.json`, `.claude-plugin/plugin.json`, or `package.json`) and directory structure to extract its description plus sorted identifiers and counts of bundled **`skills`** (`skills/*`, `.agents/skills/*`), **`hooks`** (`hooks.json`, `hooks/*`), and **`MCPs`** (`mcp_config.json`, `mcp.json`, `.mcp.json`).
- **Native Toggling & OpenCode Retention**: Pressing <kbd>Space</kbd> or <kbd>Enter</kbd> on a plugin updates its native harness configuration atomically (preserving symlinks and `.bak` backups). For OpenCode npm plugins, toggling OFF removes the package specifier from `opencode.json` while retaining `enabled = false` in `~/.config/maga/plugins.toml` so it can be toggled back ON later.
- **Complete Native Uninstall (<kbd>d</kbd>)**: Pressing <kbd>d</kbd> (with <kbd>y</kbd>/<kbd>n</kbd> confirmation) removes the plugin from the harness's native config/registry files (including Claude's `settings.json` and `installed_plugins.json`), deletes its installed directory on disk, and prunes it from `~/.config/maga/plugins.toml`.

---

## Quick Start

### Install & Run

**Option 1 — Prebuilt Release Binary (Linux, macOS, Windows)**

Download the archive for your platform (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, or `x86_64-pc-windows-msvc`) from [**GitHub Releases**](https://github.com/akrista/maga/releases), or install via [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
# Quick one-liner for Linux / macOS (adjust TARGET for your OS/arch)
TARGET="x86_64-unknown-linux-gnu"
curl -fsSL "https://github.com/akrista/maga/releases/latest/download/maga-v0.3.0-${TARGET}.tar.gz" \
  | tar -xz -C ~/.local/bin maga

# Or install prebuilt binary via cargo-binstall
cargo binstall --git https://github.com/akrista/maga maga
```

**Option 2 — Install from GitHub via Cargo (Rust 1.88+)**

```bash
cargo install --git https://github.com/akrista/maga
maga
```

**Option 3 — Build from a Local Clone**

```bash
git clone https://github.com/akrista/maga.git
cd maga
cargo install --path .
maga
```

### CLI Usage & Path Overrides

```bash
# Override individual config or harness file paths
maga \
  --config ~/.config/maga/config.toml \
  --mcp-config ~/.config/maga/mcp.toml \
  --plugins-config ~/.config/maga/plugins.toml \
  --agy-config ~/.gemini/config/mcp_config.json \
  --claude-config ~/.claude.json \
  --copilot-config ~/.copilot/mcp-config.json \
  --grok-config ~/.grok/config.toml \
  --opencode-config ~/.config/opencode/opencode.json

# Print reconciled MCP server status in plain text and exit without launching the TUI
maga --dump

# Check if harnesses are converged with mcp.toml (read-only, exit 0 = in sync, 1 = drift, 2 = conflict)
maga --check

# Synchronize canonical MCPs to all enabled harnesses non-interactively (fails fast on conflict)
maga --sync

# Authoritatively push mcp.toml to all harnesses, overriding conflicts
maga --sync --force

# Check if a newer version of maga is available on GitHub Releases
maga --check-update

# Automatically download, verify, and replace maga in-place with the latest release
maga --update
```

| Flag | Default / Behavior |
| :--- | :--- |
| `--config <CONFIG>` | Override general `maga` config path (`~/.config/maga/config.toml`) |
| `--mcp-config <MCP_CONFIG>` | Override canonical MCP registry path (`~/.config/maga/mcp.toml`) |
| `--plugins-config <PLUGINS_CONFIG>` | Override per-harness plugins ledger path (`~/.config/maga/plugins.toml`) |
| `--agy-config <AGY_CONFIG>` | Override Antigravity config path (`~/.gemini/config/mcp_config.json`) |
| `--claude-config <CLAUDE_CONFIG>` | Override Claude Code config path (`~/.claude.json`) |
| `--copilot-config <COPILOT_CONFIG>` | Override GitHub Copilot config path (`~/.copilot/mcp-config.json`) |
| `--grok-config <GROK_CONFIG>` | Override Grok CLI config path (`~/.grok/config.toml`) |
| `--opencode-config <OPENCODE_CONFIG>` | Override OpenCode config path (`~/.config/opencode/opencode.json`) |
| `--dump` | Print reconciled MCP server status (`✓` enabled, `○` disabled, `!` conflict) in plain text and exit (also used automatically when `stdout` is piped/non-TTY) |
| `--check` | Verify whether canonical state and harnesses are converged without writing any files (exit 0 = converged, 1 = drift, 2 = conflict) |
| `--sync` | Reconcile and synchronize canonical MCPs to all enabled harnesses non-interactively, failing fast on conflicts (exit 0 = synced, 2 = conflict) |
| `--force` | Combined with `--sync`: authoritatively push `mcp.toml` to all enabled harnesses, overriding conflicts |
| `--check-update` | Query GitHub Releases for a newer version of `maga` and print status without modifying files |
| `--update` | Download the matching release archive, verify its SHA-256 checksum, and upgrade the binary in-place |
| `-h`, `--help` | Print CLI help information |
| `-V`, `--version` | Print version information |

---

## Keyboard Shortcuts

### Workspace Navigation & Actions

| Key | Action |
| :--- | :--- |
| <kbd>Tab</kbd> / <kbd>Shift+Tab</kbd> · <kbd>1</kbd> / <kbd>2</kbd> | Switch active workspace section between **MCPs (`1`)** and **Plugins (`2`)** |
| <kbd>j</kbd> / <kbd>k</kbd> / <kbd>↑</kbd> / <kbd>↓</kbd> | Navigate active list (or scroll `DETAILS` tools when `t` focus is active) |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd> / <kbd>Ctrl+u</kbd> / <kbd>Ctrl+d</kbd> | Jump 10 rows up / down |
| <kbd>g</kbd> / <kbd>G</kbd> / <kbd>Home</kbd> / <kbd>End</kbd> | Jump to first / last row |
| <kbd>Space</kbd> | Toggle selected MCP server (globally) or plugin (per-harness) ON / OFF |
| <kbd>Enter</kbd> | Toggle selected item ON / OFF, or fold/unfold a harness group header in **Plugins** |
| <kbd>h</kbd> / <kbd>l</kbd> / <kbd>←</kbd> / <kbd>→</kbd> | Collapse / expand harness plugin group headers in **Plugins** view |
| <kbd>/</kbd> | Enter live-filter mode (multi-token & `key:value` search for active section) |
| <kbd>p</kbd> / <kbd>P</kbd> | Run live MCP health & `tools/list` probe on selected server (`p`) or all servers (`P`) |
| <kbd>t</kbd> | Toggle scroll focus on the `DETAILS` exposed tools pane (`MCPs` view) |
| <kbd>a</kbd> / <kbd>e</kbd> | Add a new MCP server / Edit the selected server via guided in-TUI form |
| <kbd>E</kbd> | Open the selected MCP server in `$VISUAL` / `$EDITOR` as a validated TOML snippet |
| <kbd>d</kbd> | Delete selected MCP server across harnesses or uninstall selected plugin from disk & configs (<kbd>y</kbd>/<kbd>n</kbd>) |
| <kbd>c</kbd> | Open the grouped-variant **Conflict Resolution** modal |
| <kbd>m</kbd> | Toggle the high-resolution **Maga Hero Showcase (`54×22` Unicode Braille)** panel |
| <kbd>b</kbd> | Toggle **Background-Only (`hide_ui`)** mode (hide foreground UI to view full-screen Riverflow art) |
| <kbd>o</kbd> | Open the interactive **Options (`config.toml`)** modal |
| <kbd>r</kbd> / <kbd>R</kbd> | Reload & reconcile configs from disk now / Toggle periodic auto-refresh on or off |
| <kbd>s</kbd> | Force push in-memory canonical registry to disk now |
| <kbd>?</kbd> | Open Keyboard Shortcuts & ASCII Witch splash modal |
| <kbd>q</kbd> / <kbd>Ctrl+C</kbd> | Quit `maga` |

### Modal & Editor Controls

| Modal | Keys |
| :--- | :--- |
| **Add / Edit Form (`a` / `e`)** | <kbd>Tab</kbd> / <kbd>Shift+Tab</kbd> navigate fields · <kbd>Space</kbd> / <kbd>←</kbd> / <kbd>→</kbd> toggle `stdio` vs `http` or `enabled` · <kbd>Ctrl+e</kbd> edit in `$EDITOR` · <kbd>Enter</kbd> save & sync · <kbd>Esc</kbd> cancel |
| **Conflict Resolution (`c`)** | <kbd>1</kbd>..<kbd>9</kbd> keep numbered variant & sync all enabled harnesses · <kbd>E</kbd> merge in `$EDITOR` · <kbd>s</kbd> / <kbd>Esc</kbd> skip for now |
| **Options (`o`)** | <kbd>j</kbd> / <kbd>k</kbd> / <kbd>Tab</kbd> select setting · <kbd>Space</kbd> / <kbd>←</kbd> / <kbd>→</kbd> (or <kbd>-</kbd> / <kbd>+</kbd> on interval) cycle value · <kbd>Enter</kbd> save to `config.toml` · <kbd>Esc</kbd> cancel |

---

## Live Filtering & Search Syntax

Press <kbd>/</kbd> in the workspace to filter the active section (**MCPs** or **Plugins**) in real time (<kbd>Enter</kbd> locks the filter, <kbd>Esc</kbd> clears it). Multiple whitespace-separated tokens are combined with logical `AND`.

### MCP Servers (`1`) Filter Qualifiers

| Qualifier | Target Field | Examples |
| :--- | :--- | :--- |
| *(unprefixed)* | Name, description, status, transport, or endpoint | `enabled stdio npx`, `conflict`, `context7` |
| `is:<state>` / `status:<state>` | Server state (`on`/`enabled`/`active`, `off`/`disabled`/`inactive`, `conflict`/`conflicted`) | `is:enabled`, `is:off`, `status:conflict` |
| `type:<kind>` / `transport:<kind>` | Transport protocol (`stdio` or `http`) | `type:stdio`, `transport:http` |
| `cmd:<text>` / `url:<text>` / `endpoint:<text>` | Command + arguments (`stdio`) or URL (`http`) | `cmd:npx`, `url:mcp.context7.com` |
| `name:<text>` | Server identifier only | `name:sqlite-mcp`, `name:github` |
| `desc:<text>` / `description:<text>` | Server description / notes only | `desc:docs`, `description:laravel` |

### Harness Plugins (`2`) Filter Qualifiers

| Qualifier | Target Field | Examples |
| :--- | :--- | :--- |
| *(unprefixed)* | Plugin ID, display name, harness, kind, source, description, or bundled item names | `claude`, `npm`, `openspec` |
| `is:<state>` / `status:<state>` | Plugin state (`on`/`enabled`/`active`, `off`/`disabled`/`inactive`) | `is:enabled`, `is:off` |
| `harness:<name>` / `tool:<name>` | Owning harness (`antigravity`/`agy`, `claude`, `copilot`, `grok`, `opencode`) | `harness:claude`, `tool:opencode` |
| `type:<kind>` / `kind:<kind>` / `source:<text>` | Plugin kind (`plugin`, `npm`, `local`) or source/marketplace | `type:npm`, `source:official` |
| `name:<text>` / `id:<text>` | Plugin identifier or display name | `name:superpowers`, `id:context7` |
| `desc:<text>` / `description:<text>` | Plugin manifest description | `desc:workflow`, `description:git` |
| `skill:<text>` / `hook:<text>` / `mcp:<text>` | Bundled skill, hook, or MCP server identifier | `skill:brainstorming`, `hook:pre-tool`, `mcp:playwright` |

---

## Configuration Files

`maga` cleanly separates canonical MCP server definitions (`mcp.toml`), per-harness plugin state (`plugins.toml`), and general UI/tool settings (`config.toml`).

> [!NOTE]
> If `~/.config/maga/mcp.toml` does not exist on startup and `~/.config/maga/config.toml` contains a legacy `[mcps]` table, `maga` automatically migrates those servers into `mcp.toml` and cleans up `config.toml`.

### 1. Canonical MCP Registry (`~/.config/maga/mcp.toml`)

```toml
[mcps.context7]
enabled = true
description = "Up-to-date library documentation lookup"
transport = "http"
url = "https://mcp.context7.com/mcp"

[mcps.context7.headers]
CONTEXT7_API_KEY = "your-api-key"

[mcps.filesystem]
enabled = true
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]

[mcps.filesystem.env]
NODE_ENV = "production"
```

### 2. Per-Harness Plugin Ledger (`~/.config/maga/plugins.toml`)

Automatically maintained as `maga` discovers and toggles plugins across enabled harnesses, and used to retain disabled OpenCode package specifiers so they can be re-enabled at any time:

```toml
[claude."superpowers@superpowers-marketplace"]
enabled = true
kind = "bundle"
source = "superpowers-marketplace"
version = "4.1.1"

[opencode."opencode-antigravity-auth@1.6.0"]
enabled = false
kind = "npm"
source = "opencode.json"
version = "1.6.0"
```

### 3. Application Configuration (`~/.config/maga/config.toml`)

Press <kbd>o</kbd> inside the TUI to modify and persist these settings interactively, or edit `~/.config/maga/config.toml` directly. Missing standard keys are auto-populated on startup while preserving custom user keys:

```toml
# [ui] Visual theme, 3-tier mascot layers, and micro-animations
[ui]
theme = "blush"        # "blush" (Warm Sunset, default) | "indigo" (Arcane Night) | "elite" (Celestial Periwinkle)
mascot_mode = "lite"   # "lite" (compact ASCII splash in ?, default) | "rich" (Moe/Chibi Kaomoji Companion Box)
background = false     # Full-TUI Riverflow Unicode Braille background watermark on unoccupied space
hero_mascot = true     # High-resolution (54x22) Unicode Braille Hero Showcase toggled via 'm'
hide_ui = false        # Hide foreground UI panels to view only the full-screen background ('b')
animations = true      # Arcane Braille flow/glitch waves, gaze tracking, tachyonfx shaders, and sparkles

# [refresh] Periodic disk polling for external harness changes
[refresh]
enabled = true
interval_secs = 5

# [tools.<harness>] Enable/disable synchronization per harness or override its config path
[tools.antigravity]
enabled = true
config_path = "~/.gemini/config/mcp_config.json"

[tools.claude]
enabled = true
config_path = "~/.claude.json"

[tools.copilot]
enabled = true
config_path = "~/.copilot/mcp-config.json"

[tools.grok]
enabled = true
config_path = "~/.grok/config.toml"

[tools.opencode]
enabled = true
config_path = "~/.config/opencode/opencode.json"
```

---

## 3-Tier Spellcaster Mascot & Terminal Compatibility

<table>
  <tr>
    <td align="center" width="50%">
      <img src="spellcaster.png" alt="Tier 1 Riverflow Spellcaster Source" width="240" /><br />
      <strong>Tier 1 · Full-TUI Riverflow Background (<code>spellcaster.png</code>)</strong><br />
      <sub><code>80×32</code> cells (<code>160×128</code> Braille sub-pixels) · Toggle with <kbd>b</kbd></sub>
    </td>
    <td align="center" width="50%">
      <img src="maga.png" alt="Tier 2 Hero Mascot Source" width="240" /><br />
      <strong>Tier 2 · Hero Mascot Showcase (<code>maga.png</code>)</strong><br />
      <sub><code>54×22</code> cells (<code>108×88</code> Braille sub-pixels) · Toggle with <kbd>m</kbd></sub>
    </td>
  </tr>
</table>

<details open>
<summary><strong>Visual Tiers &amp; Autonomous Gaze Controller</strong></summary>
<br />

`maga`'s visual system is driven by a unified keyboard-focus & autonomous idle gaze controller (`gaze_x`, `gaze_y`) and an action-reactive state machine (`Idle`, `Casting`, `Scrying`, `Startled`, `Refreshing`) without capturing mouse input:

1. **Full-TUI Riverflow Background Watermark (`ui.background`)**: Samples `spellcaster.png` into an `80×32` cell (`160×128` virtual sub-pixel) **Unicode Braille (`⠁..⣿`)** grid with unsharp-mask contrast and `4×4` Bayer dithering. In normal workspace mode, dimmed Braille dots render only into unoccupied background cells with a 2-column horizontal breathing margin from foreground text/borders. Pressing <kbd>b</kbd> (`ui.hide_ui`) reveals the full-screen True-Color RGB canvas with regional sub-pixel gaze/blinking, wand float, and arcane rune rings.
2. **Hero Mascot Showcase (`ui.hero_mascot`, <kbd>m</kbd> key)**: High-resolution (`54×22` cells = `108×88` virtual sub-pixels) Unicode Braille + arcane shader portrait of `maga.png` featuring `4×4` Bayer dithering, radial startup `reveal`, sine-wave `flow` shimmer, periodic scanline `glitch` bands, pupil gaze tracking, eyelid blinking, cat-eye glasses glint sweeps, sunflower wand recoil bursts, and `tachyonfx` transitions.
3. **Moe / Chibi Kaomoji Companion (`ui.mascot_mode = "rich" | "lite"`)**: In `"rich"` mode, renders a compact (`~14×5` cells) lower-right `COMPANION · Maga` panel with gaze- and action-reactive faces (`*=[o‿o]=*` idle, `*=[-‿-]=*` blink, `*=[✧‿<]=*` casting, `*=[o_O]=?` scrying filter, `*=[>_<]=!` conflict/alert) and status dialogue. In `"lite"` mode (default), `DETAILS` uses the full column height and the ASCII/Unicode witch splash appears inside the <kbd>?</kbd> Help modal.

</details>

<details open>
<summary><strong>Responsive Breakpoints & Accessibility (`NO_COLOR`)</strong></summary>
<br />

| Viewport / Environment | Layout & Rendering Behavior |
| :--- | :--- |
| **`>= 60×22` (TrueColor / 256-Color)** | Full master-detail split layout, optional lower-right `COMPANION · Maga` box (`mascot_mode = "rich"`), Unicode Braille canvases, and ~30–60 FPS `tachyonfx` transitions |
| **`60×20` to `60×21`** | Master-detail split layout; automatically collapses the lower-right companion panel so `DETAILS` stays readable |
| **`50×16` to `59×N`** | Single-column compact layout |
| **`< 50×16`** | Centered minimum-size notice |
| **`NO_COLOR=1` or `TERM=dumb`** | Strips ANSI RGB colors, freezes idle animations and `tachyonfx` buffer effects, and uses monochrome attributes with explicit status symbols (`✓` / `[x]`, `[ ]`, `!`, `✗`) |

</details>

---

## License

Released under the [MIT License](LICENSE).
