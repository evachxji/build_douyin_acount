# 一次性登录态快照：把用户 Chrome 的登录相关文件（Cookies / Local Storage）
# 克隆到应用的独立浏览器 Profile，供发布 sidecar 复用登录态。
#
# 前提：Chrome 必须完全关闭（Cookies 文件被 Chrome 独占锁定时无法复制）。
# 用法：powershell -ExecutionPolicy Bypass -File snapshot_chrome_profile.ps1

$ErrorActionPreference = "Stop"

$userData = Join-Path $env:LOCALAPPDATA "Google\Chrome\User Data"
if (-not (Test-Path (Join-Path $userData "Local State"))) {
    Write-Error "未找到 Chrome 配置目录: $userData"
    exit 1
}

# Chrome 锁检测：存在 Lockfile 表示 Chrome 正在运行。
if (Test-Path (Join-Path $userData "lockfile")) {
    Write-Error "Chrome 正在运行。请先完全退出 Chrome（托盘图标也要退出），再执行本脚本。"
    exit 1
}

# 输出目标：应用独立 Profile（与 sidecar 的 profileDir 一致）。
$target = Join-Path $PSScriptRoot "..\..\..\storage\browser\douyin-profile"
if (Test-Path $target) {
    Remove-Item -LiteralPath $target -Recurse -Force
}
$targetDefault = Join-Path $target "Default"
New-Item -ItemType Directory -Force -Path (Join-Path $targetDefault "Network") | Out-Null

# Local State：含 DPAPI 加密的 cookie 密钥（同 Windows 账户可解密）。
Copy-Item -LiteralPath (Join-Path $userData "Local State") -Destination (Join-Path $target "Local State") -Force

# 登录态核心：Cookies（含 douyin.com 会话）。
$srcCookies = Join-Path $userData "Default\Network\Cookies"
if (Test-Path $srcCookies) {
    Copy-Item -LiteralPath $srcCookies -Destination (Join-Path $targetDefault "Network\Cookies") -Force
    if (Test-Path "$srcCookies-journal") {
        Copy-Item -LiteralPath "$srcCookies-journal" -Destination (Join-Path $targetDefault "Network\Cookies-journal") -Force
    }
}

# Local Storage（部分站点登录态依赖；leveldb 可能不存在则跳过）。
$srcLs = Join-Path $userData "Default\Local Storage\leveldb"
if (Test-Path $srcLs) {
    Copy-Item -Path $srcLs -Destination (Join-Path $targetDefault "Local Storage\leveldb") -Recurse -Force
}

Write-Output "快照完成：$target"
Write-Output "请在 30 秒后重新打开 Chrome 正常工作，发布窗口将始终独立且带登录态。"
