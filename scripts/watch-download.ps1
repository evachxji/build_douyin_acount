param(
    [int]$CheckSeconds = 60,
    [int]$StallLimit = 3,
    [int]$MaxRestarts = 999
)

# 下载看门狗：进程死亡自动重启；进度停滞超过 StallLimit 个检查周期强制重启。
$scriptPath = Join-Path $PSScriptRoot "pull-ollama-manual.py"
$workDir = Split-Path -Parent $PSScriptRoot
$blobDir = Join-Path $env:USERPROFILE ".ollama\models\blobs"
$logFile = Join-Path $env:TEMP "ollama_watchdog.log"
$pullLog = Join-Path $env:TEMP "ollama_manual_watch.log"

"watchdog start $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" | Out-File $logFile -Append

$lastSize = (Get-ChildItem $blobDir -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
$stall = 0
$restarts = 0

while ($restarts -lt $MaxRestarts) {
    Start-Sleep -Seconds $CheckSeconds
    $proc = Get-Process -Name python -ErrorAction SilentlyContinue |
        Where-Object { $_.StartTime -gt (Get-Date).AddMinutes(-90) }
    if (-not $proc) {
        "restart (dead) $(Get-Date -Format 'HH:mm:ss')" | Out-File $logFile -Append
        Start-Process -FilePath "python" -ArgumentList "-u",$scriptPath -WorkingDirectory $workDir `
            -RedirectStandardOutput $pullLog -RedirectStandardError "$pullLog.err" -WindowStyle Hidden
        $restarts++
        $stall = 0
        Start-Sleep -Seconds 5
    }
    $size = (Get-ChildItem $blobDir -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    $deltaMB = [math]::Round(($size - $lastSize) / 1MB, 0)
    if ($deltaMB -eq 0) { $stall++ } else { $stall = 0 }
    if ($stall -ge $StallLimit) {
        "forced restart (stall) $(Get-Date -Format 'HH:mm:ss') size=$([math]::Round($size/1GB,2))GB" |
            Out-File $logFile -Append
        Get-Process -Name python -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        Start-Process -FilePath "python" -ArgumentList "-u",$scriptPath -WorkingDirectory $workDir `
            -RedirectStandardOutput $pullLog -RedirectStandardError "$pullLog.err" -WindowStyle Hidden
        $restarts++
        $stall = 0
    }
    $lastSize = $size
}
