# herdr-auto-update.ps1
# Daily check for a new herdr release. If one is out: rebase the kit branch onto it, build, test,
# install the new custom herdr.exe (old one kept as herdr.old-HHmmss.exe), push kit to the fork,
# and notify in herdr. On a conflict or a failed build/test it rolls back, changes nothing, and notifies.
# Usage: herdr-auto-update.ps1 [-DryRun]   (DryRun only reports whether an update is available)
param([switch]$DryRun)
$ErrorActionPreference = 'Continue'

$repo     = Join-Path $env:USERPROFILE '.herdr\src\herdr'
$custom   = Join-Path $env:USERPROFILE '.herdr\custom'
$herdr    = Join-Path $custom 'herdr.exe'
$cargo    = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$here     = Join-Path $env:USERPROFILE '.herdr\kit\auto-update'
$log      = Join-Path $here 'auto-update.log'
$buildLog = Join-Path $here 'last-build.log'
$upstream = 'herdrdev/herdr'
$fork     = 'MAISTRY/herdr'

function Log([string]$m) {
  "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  $m" | Add-Content -Path $log -Encoding utf8
}
function Notify([string]$title, [string]$body) {
  Log "NOTIFY: $title | $body"
  & $herdr notification show $title --body $body --sound request *> $null
}
function G {  # run git in the repo, return trimmed stdout; $LASTEXITCODE is left for the caller
  $out = & git -C $repo @args 2>$null
  if ($out) { ($out -join "`n").Trim() } else { '' }
}
function ToVersion([string]$tag) {
  try { [version]($tag.TrimStart('v')) } catch { $null }
}

Set-Location $repo
Log '--- check start'

if (G status --porcelain) {
  Notify 'herdr update skipped' 'Uncommitted changes in ~/.herdr/src/herdr. Commit or stash them, then say "update herdr".'
  exit 1
}

$base = G describe --tags --abbrev=0 kit
$latest = (& gh api "repos/$upstream/releases/latest" -q .tag_name 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $latest) { Log 'could not read latest release from GitHub; will retry next run'; exit 0 }
$latest = $latest.Trim()

$vBase = ToVersion $base; $vLatest = ToVersion $latest
if (-not $vBase -or -not $vLatest) { Log "cannot compare versions base=$base latest=$latest"; exit 0 }
if ($vLatest -le $vBase) { Log "up to date ($base)"; exit 0 }

Log "new release $latest (kit is on $base)"
if ($DryRun) { Log 'dry run: stopping here'; Write-Output "update available: $base -> $latest"; exit 0 }

$startBranch = G rev-parse --abbrev-ref HEAD
$oldKit = G rev-parse kit
G fetch origin --tags | Out-Null
if ($LASTEXITCODE -ne 0) { Log 'git fetch failed; will retry next run'; exit 0 }

$backup = "kit-before-$latest"
G branch -f $backup kit | Out-Null
G checkout -q kit | Out-Null

function RollBack([string]$why) {
  G rebase --abort | Out-Null
  G checkout -q kit | Out-Null
  G reset -q --hard $oldKit | Out-Null
  if ($startBranch -and $startBranch -ne 'kit') { G checkout -q $startBranch | Out-Null }
  Notify "herdr $latest needs a hand" "$why Nothing was changed. Say `"update herdr`" to Claude."
  exit 1
}

G rebase --onto $latest $base kit | Out-Null
if ($LASTEXITCODE -ne 0) { RollBack "Your kit changes conflict with $latest." }

Log 'building (profile fast)'
cmd /c "`"$cargo`" build --profile fast > `"$buildLog`" 2>&1"
if ($LASTEXITCODE -ne 0) { RollBack "Build failed on $latest (see ~/.herdr/kit/auto-update/last-build.log)." }

Log 'testing'
cmd /c "`"$cargo`" test --profile fast >> `"$buildLog`" 2>&1"
if ($LASTEXITCODE -ne 0) { RollBack "Tests failed on $latest (see ~/.herdr/kit/auto-update/last-build.log)." }

# Install: a running exe can be renamed on Windows, so keep the old one as a backup and copy the new one in.
$built = Join-Path $repo 'target\fast\herdr.exe'
$old = Join-Path $custom ("herdr.old-{0}.exe" -f (Get-Date -Format 'HHmmss'))
try {
  Rename-Item -Path $herdr -NewName (Split-Path $old -Leaf) -ErrorAction Stop
  Copy-Item -Path $built -Destination $herdr -ErrorAction Stop
} catch {
  if (-not (Test-Path $herdr) -and (Test-Path $old)) { Rename-Item -Path $old -NewName 'herdr.exe' }
  RollBack "Could not install the new herdr.exe: $($_.Exception.Message)."
}
Log "installed $latest; previous exe kept as $(Split-Path $old -Leaf)"

# Rebase rewrote kit, so the push needs a lease on the old tip.
G push -q "--force-with-lease=kit:$oldKit" fork kit | Out-Null
$pushNote = if ($LASTEXITCODE -eq 0) { 'Pushed kit to your fork.' } else { 'Push to your fork failed; run: git push --force-with-lease fork kit' }
& gh repo sync $fork -b master *> $null
if ($startBranch -and $startBranch -ne 'kit') { G checkout -q $startBranch | Out-Null }

Notify "herdr updated to $latest" "Restart the herdr server to use it. Old exe kept as $(Split-Path $old -Leaf). $pushNote"
Log '--- done'
exit 0
