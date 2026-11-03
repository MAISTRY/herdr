# Writes one line of system stats for herdr's tab bar every 2 seconds:
#   CPU 12% · RAM 5.8/7.4G · C: 61% · GPU 3%
# herdr reads it with `type stats.txt` (see tab_bar_right in %APPDATA%\herdr\config.toml).
# Started hidden at logon by the "herdr-stats" shortcut in the Startup folder.

$out = Join-Path $PSScriptRoot 'stats.txt'
$tmp = "$out.tmp"

# One instance only.
$mutex = New-Object System.Threading.Mutex($false, 'Global\herdr-stats')
if (-not $mutex.WaitOne(0)) { exit }

$counters = @('\Processor(_Total)\% Processor Time', '\GPU Engine(*engtype_3D)\Utilization Percentage')
$diskTick = 0
$disk = ''

Get-Counter -Counter $counters -SampleInterval 2 -Continuous -ErrorAction SilentlyContinue | ForEach-Object {
    $s = $_.CounterSamples
    $cpu = [math]::Round(($s | Where-Object Path -like '*processor(_total)*').CookedValue)
    $gpu = [math]::Round((($s | Where-Object Path -like '*gpu engine*' | Measure-Object CookedValue -Sum).Sum))
    if ($gpu -gt 100) { $gpu = 100 }

    $os = Get-CimInstance Win32_OperatingSystem -Property TotalVisibleMemorySize, FreePhysicalMemory
    $totalG = $os.TotalVisibleMemorySize / 1MB
    $usedG = ($os.TotalVisibleMemorySize - $os.FreePhysicalMemory) / 1MB

    if ($diskTick -le 0) {
        $c = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" -Property Size, FreeSpace
        $disk = [math]::Round((1 - $c.FreeSpace / $c.Size) * 100)
        $diskTick = 15   # every 30 s
    }
    $diskTick--

    $dot = ' ' + [char]0xB7 + ' '   # middle dot, kept out of the file so PowerShell 5.1 reads it right
    $line = ('CPU {0}%' + $dot + 'RAM {1:N1}/{2:N1}G' + $dot + 'C: {3}%' + $dot + 'GPU {4}%') -f $cpu, $usedG, $totalG, $disk, $gpu
    [IO.File]::WriteAllText($tmp, $line, [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $tmp -Destination $out -Force
}
