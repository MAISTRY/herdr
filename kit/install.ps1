# install.ps1
# Sets up the herdr kit on a new Windows machine from a checkout of the fork's kit branch:
#   1. kit scripts       -> %USERPROFILE%\.herdr\kit   (ctrl+t Claude tab, focus-space, subagents, ...)
#   2. stats             -> %USERPROFILE%\.herdr\stats (CPU/RAM bar) + Startup shortcut, started now
#   3. config.toml       -> %APPDATA%\herdr\config.toml (all key bindings, theme, sidebar, tab bar)
#   4. Claude worktrees plugin linked into herdr
#   5. Windows Terminal: ctrl+alt+1..9 jump to space 1..9
#   6. with -Build: builds herdr from this branch and installs it as %USERPROFILE%\.herdr\custom\herdr.exe
# Every file it replaces is first copied to <file>.bak-<time>.
#
# Usage (from the repo root, on the kit branch):
#   powershell -NoProfile -ExecutionPolicy Bypass -File kit\install.ps1 [-Space2Name <name>] [-Build] [-SkipTerminal]
# -Space2Name: the space whose name shows purple in the sidebar (asked for when not given).
# Needs: Node.js on PATH. -Build also needs Rust (cargo) and Zig under %LOCALAPPDATA%\Programs\zig.
param([string]$Space2Name, [switch]$Build, [switch]$SkipTerminal)
$ErrorActionPreference = 'Stop'

$kit      = $PSScriptRoot
$repo     = Split-Path $kit -Parent
$userHome = $env:USERPROFILE
$dest     = Join-Path $userHome '.herdr\kit'
$stats    = Join-Path $userHome '.herdr\stats'
$custom   = Join-Path $userHome '.herdr\custom'
$cfgDir   = Join-Path $env:APPDATA 'herdr'
$stamp    = Get-Date -Format 'yyyyMMdd-HHmmss'
$utf8     = New-Object System.Text.UTF8Encoding($false)

function Save-Backup([string]$path) {
  if (Test-Path $path) { Copy-Item $path "$path.bak-$stamp"; Write-Host "  backed up $path" }
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw 'node is not on PATH. Install Node.js first: the kit shortcuts run node scripts.'
}

New-Item -ItemType Directory -Force $dest, $stats, $custom, $cfgDir, (Join-Path $userHome '.herdr\worktrees') | Out-Null

# 1. kit scripts (not the config template, this installer, the stats folder or the patches)
Write-Host '1. kit scripts'
$skip = @('config.toml', 'install.ps1', 'stats', 'patches')
Get-ChildItem $kit | Where-Object { $skip -notcontains $_.Name } |
  ForEach-Object { Copy-Item $_.FullName -Destination $dest -Recurse -Force }
Write-Host "  -> $dest"

# 2. stats for the tab bar
Write-Host '2. stats'
Copy-Item (Join-Path $kit 'stats\*') $stats -Force
& (Join-Path $stats 'add-herdr-stats-startup.ps1') | ForEach-Object { Write-Host "  $_" }
Start-Process powershell.exe -WindowStyle Hidden -ArgumentList @(
  '-NoProfile', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-File', (Join-Path $stats 'stats.ps1'))
Write-Host "  -> $stats (running)"

# 3. config.toml with this machine's user folder and the purple space's name
Write-Host '3. config.toml'
if (-not $Space2Name) { $Space2Name = Read-Host '  Name of the space shown purple in the sidebar' }
$cfg = Join-Path $cfgDir 'config.toml'
Save-Backup $cfg
$text = [IO.File]::ReadAllText((Join-Path $kit 'config.toml'), $utf8)
$text = $text.Replace('{{HOME}}', $userHome).Replace('{{SPACE2}}', $Space2Name)
[IO.File]::WriteAllText($cfg, $text, $utf8)
Write-Host "  -> $cfg"

# 6 (before 4, so the plugin is linked with the new build). Build herdr from this branch.
$herdr = Join-Path $custom 'herdr.exe'
if ($Build) {
  Write-Host '6. build herdr (cargo build --profile fast)'
  if (-not $env:ZIG) {
    $z = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Programs\zig') -Filter zig.exe -Recurse -ErrorAction SilentlyContinue |
         Sort-Object FullName -Descending | Select-Object -First 1
    if ($z) { $env:ZIG = $z.FullName } else { throw 'Zig not found under %LOCALAPPDATA%\Programs\zig. Install it or set ZIG.' }
  }
  Push-Location $repo
  try { cargo build --profile fast; if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" } }
  finally { Pop-Location }
  Save-Backup $herdr
  Copy-Item (Join-Path $repo 'target\fast\herdr.exe') $herdr -Force
  Write-Host "  -> $herdr"
}

# 4. Claude worktrees plugin
Write-Host '4. plugin kit.claude-worktrees'
if (-not (Test-Path $herdr)) {
  $onPath = Get-Command herdr -ErrorAction SilentlyContinue
  $herdr = if ($onPath) { $onPath.Source } else { $null }
}
if ($herdr) {
  & $herdr plugin link (Join-Path $dest 'claude-worktrees')
  if ($LASTEXITCODE -ne 0) { Write-Host '  link failed or already linked; check with: herdr plugin list' }
} else {
  Write-Host '  herdr not found; after installing it run: herdr plugin link "%USERPROFILE%\.herdr\kit\claude-worktrees"'
}

# 5. Windows Terminal ctrl+alt+1..9 -> prefix (ctrl+b) then alt+N
if (-not $SkipTerminal) {
  Write-Host '5. Windows Terminal keys'
  $wt = @(
    (Join-Path $env:LOCALAPPDATA 'Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json'),
    (Join-Path $env:LOCALAPPDATA 'Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json'),
    (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows Terminal\settings.json')
  ) | Where-Object { Test-Path $_ }
  if (-not $wt) { Write-Host '  settings.json not found: open Windows Terminal once, then run this again.' }
  $merge = Join-Path $env:TEMP "herdr-wt-merge-$stamp.js"
  [IO.File]::WriteAllText($merge, @'
const fs = require("fs");
const file = process.argv[2];
const raw = fs.readFileSync(file, "utf8").replace(/^\uFEFF/, "");
// settings.json may hold comments and trailing commas: drop them outside strings
let s = "", i = 0, inStr = false;
while (i < raw.length) {
  const c = raw[i], d = raw[i + 1];
  if (inStr) { s += c; if (c === "\\") { s += d; i += 2; continue; } if (c === '"') inStr = false; i++; continue; }
  if (c === '"') { inStr = true; s += c; i++; continue; }
  if (c === "/" && d === "/") { while (i < raw.length && raw[i] !== "\n") i++; continue; }
  if (c === "/" && d === "*") { const e = raw.indexOf("*/", i + 2); i = e < 0 ? raw.length : e + 2; continue; }
  s += c; i++;
}
const j = JSON.parse(s.replace(/,(\s*[}\]])/g, "$1"));
const ours = (x) => /^User\.herdrSpace\d$/.test((x && x.id) || "") || /^ctrl\+alt\+[1-9]$/.test((x && x.keys) || "");
j.actions = (j.actions || []).filter((a) => !ours(a));
j.keybindings = (j.keybindings || []).filter((k) => !ours(k));
for (let n = 1; n <= 9; n++) {
  j.actions.push({ command: { action: "sendInput", input: "\u0002\u001b" + n }, id: "User.herdrSpace" + n });
  j.keybindings.push({ id: "User.herdrSpace" + n, keys: "ctrl+alt+" + n });
}
fs.writeFileSync(file, JSON.stringify(j, null, 4) + "\n");
'@, $utf8)
  foreach ($f in $wt) {
    Save-Backup $f
    node $merge $f
    if ($LASTEXITCODE -ne 0) { Write-Host "  could not update $f (left as it was)" } else { Write-Host "  -> $f" }
  }
  Remove-Item $merge
}

Write-Host ''
Write-Host 'Done. Restart herdr (and Windows Terminal) to load the new keys.'
if (-not $Build -and -not (Test-Path (Join-Path $custom 'herdr.exe'))) {
  Write-Host 'Some settings (space colours, agents grouped by space) need the kit build: run again with -Build.'
}
Write-Host 'Not covered here: the Claude Code hooks that call herdr-subagents.js (in %USERPROFILE%\.claude\settings.json).'
