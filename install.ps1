#!/usr/bin/env bash
# Polyglot installer for maga: Bash (Linux/macOS) and PowerShell (Windows).
# Installs the prebuilt GitHub Release binary. Upgrades are delegated to `maga --update`.
#
#   Linux/macOS: curl -fsSL https://raw.githubusercontent.com/akrista/maga/master/install.ps1 | bash
#   Windows:     irm https://raw.githubusercontent.com/akrista/maga/master/install.ps1 | iex
#
# Flags: --force | -f (reinstall even if maga exists), --version vX.Y.Z (pin a release)
# Env:   MAGA_VERSION, MAGA_INSTALL_DIR (default: ~/.local/bin or %LOCALAPPDATA%\maga\bin)

function REM() { return; }
REM @'
REM '; : << "BASH"
BASH

set -eu

REPO="akrista/maga"
FORCE=false
VERSION="${MAGA_VERSION:-}"
INSTALL_DIR="${MAGA_INSTALL_DIR:-$HOME/.local/bin}"

while [ $# -gt 0 ]; do
    case "$1" in
        --force|-f) FORCE=true ;;
        --version) VERSION="${2:-}"; shift ;;
    esac
    shift
done

info() { printf '\033[0;34m[INFO]\033[0m %s\n' "$1"; }
ok()   { printf '\033[0;32m[SUCCESS]\033[0m %s\n' "$1"; }
warn() { printf '\033[0;33m[WARNING]\033[0m %s\n' "$1"; }
die()  { printf '\033[0;31m[ERROR]\033[0m %s\n' "$1" >&2; exit 1; }

# Already installed: let maga upgrade itself (verified, in-place), unless forced.
if [ "$FORCE" = false ] && [ -z "$VERSION" ] && command -v maga >/dev/null 2>&1; then
    info "maga $(maga --version 2>/dev/null | awk '{print $2}') found; checking for updates..."
    exec maga --update
fi

[ -n "${TERMUX_VERSION:-}" ] && die "Termux has no prebuilt binary (bionic libc). Use: cargo install --git https://github.com/$REPO --locked"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)             TARGET="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64|Linux-arm64) TARGET="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64)            TARGET="x86_64-apple-darwin" ;;
    Darwin-arm64|Darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
    *) die "Unsupported platform: $(uname -s)-$(uname -m)" ;;
esac

command -v curl >/dev/null 2>&1 || die "curl is required."

if [ -z "$VERSION" ]; then
    # /releases/latest redirects to /releases/tag/<tag>: no API quota, no JSON parsing.
    VERSION="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")"
    VERSION="${VERSION##*/}"
fi
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
[ "$VERSION" = "vlatest" ] && die "Could not resolve the latest release."

ARCHIVE="maga-$VERSION-$TARGET.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info "Downloading $ARCHIVE..."
curl -fsSL -o "$TMP/$ARCHIVE" "$BASE/$ARCHIVE" || die "Download failed: $BASE/$ARCHIVE"
curl -fsSL -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS" || die "Could not download SHA256SUMS."

EXPECTED="$(awk -v f="$ARCHIVE" '$2 == f {print $1}' "$TMP/SHA256SUMS")"
[ -n "$EXPECTED" ] || die "No checksum listed for $ARCHIVE."
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP/$ARCHIVE" | awk '{print $1}')"
else
    ACTUAL="$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{print $1}')"
fi
[ "$EXPECTED" = "$ACTUAL" ] || die "SHA-256 mismatch for $ARCHIVE (expected $EXPECTED, got $ACTUAL)."
ok "Checksum verified."

tar -xzf "$TMP/$ARCHIVE" -C "$TMP" maga
mkdir -p "$INSTALL_DIR"
# Atomic replace: safe even if an agent is running the old binary.
install -m 755 "$TMP/maga" "$INSTALL_DIR/maga.new"
mv -f "$INSTALL_DIR/maga.new" "$INSTALL_DIR/maga"
ok "maga $VERSION installed to $INSTALL_DIR/maga"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) warn "$INSTALL_DIR is not in PATH. Add: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
info "Future upgrades: maga --update"
exit
'@
# '

# PowerShell installer for Windows

function Write-LogInfo ($message)    { Write-Host "[INFO] $message" -ForegroundColor Blue }
function Write-LogSuccess ($message) { Write-Host "[SUCCESS] $message" -ForegroundColor Green }
function Write-LogWarning ($message) { Write-Host "[WARNING] $message" -ForegroundColor Yellow }
function Write-LogError ($message)   { Write-Host "[ERROR] $message" -ForegroundColor Red }

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is very slow with the progress bar

$Repo = "akrista/maga"
$Force = $args -contains "--force" -or $args -contains "-f"
$Version = $env:MAGA_VERSION
$versionIndex = [array]::IndexOf($args, "--version")
if ($versionIndex -ge 0 -and $versionIndex + 1 -lt $args.Count) { $Version = $args[$versionIndex + 1] }
$InstallDir = if ($env:MAGA_INSTALL_DIR) { $env:MAGA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "maga\bin" }

# Already installed: let maga upgrade itself (verified, in-place), unless forced.
$existing = Get-Command maga -ErrorAction SilentlyContinue
if ($existing -and -not $Force -and -not $Version) {
    Write-LogInfo "maga found at $($existing.Source); checking for updates..."
    & $existing.Source --update
    return
}

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    Write-LogError "Unsupported architecture: $($env:PROCESSOR_ARCHITECTURE). Only x86_64 has a prebuilt Windows binary."
    return
}
$Target = "x86_64-pc-windows-msvc"

if (-not $Version) {
    $Version = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest").tag_name
}
if (-not $Version.StartsWith("v")) { $Version = "v$Version" }

$Archive = "maga-$Version-$Target.zip"
$Base = "https://github.com/$Repo/releases/download/$Version"
$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("maga-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Tmp -Force | Out-Null

try {
    Write-LogInfo "Downloading $Archive..."
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Archive" -OutFile (Join-Path $Tmp $Archive)
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS" -OutFile (Join-Path $Tmp "SHA256SUMS")

    $expected = ((Get-Content (Join-Path $Tmp "SHA256SUMS")) |
        Where-Object { $_ -match "\s$([regex]::Escape($Archive))$" } |
        Select-Object -First 1) -split "\s+" | Select-Object -First 1
    if (-not $expected) { throw "No checksum listed for $Archive." }
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $Tmp $Archive)).Hash.ToLower()
    if ($expected.ToLower() -ne $actual) { throw "SHA-256 mismatch for $Archive (expected $expected, got $actual)." }
    Write-LogSuccess "Checksum verified."

    Expand-Archive -Path (Join-Path $Tmp $Archive) -DestinationPath $Tmp -Force
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $dest = Join-Path $InstallDir "maga.exe"

    # A running exe (e.g. an agent's MCP server) cannot be overwritten, but it can be renamed.
    if (Test-Path -LiteralPath $dest) {
        $old = "$dest.old"
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $dest -Destination $old -Force
    }
    Copy-Item -LiteralPath (Join-Path $Tmp "maga.exe") -Destination $dest -Force
    Remove-Item -LiteralPath "$dest.old" -Force -ErrorAction SilentlyContinue   # best effort: still locked if running
    Write-LogSuccess "maga $Version installed to $dest"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($userPath -split ";") -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$userPath", "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-LogInfo "Added $InstallDir to your user PATH (restart open terminals)."
    }

    $others = Get-Command maga -All -ErrorAction SilentlyContinue | Where-Object { $_.Source -ne $dest }
    foreach ($other in $others) {
        Write-LogWarning "Another maga is on PATH: $($other.Source). Remove it (e.g. cargo uninstall maga) to avoid shadowing."
    }
    Write-LogInfo "Future upgrades: maga --update"
}
catch {
    Write-LogError $_.Exception.Message
}
finally {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}
