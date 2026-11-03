# Adds the "herdr-stats" shortcut to the Startup folder so the CPU/RAM bar restarts after every reboot.
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\herdr-stats.lnk'
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$s.Arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $env:USERPROFILE '.herdr\stats\stats.ps1') + '"'
$s.WindowStyle = 7
$s.Save()
"created $lnk"
