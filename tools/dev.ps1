# 片羽开发启动：仅为本脚本的子进程补 .cargo/bin PATH，不修改永久环境、执行策略或审批设置。
# 用法（沿用现有执行策略调用）：
#   powershell -File tools\dev.ps1 [-DataDir <完整绝对路径>]
# 若当前执行策略阻止运行本脚本，会看到脚本被禁止运行的报错；请记录具体报错交回处理，
# 不要使用 -ExecutionPolicy Bypass 或 Set-ExecutionPolicy 绕过。
param(
    [string]$DataDir = (Join-Path 'D:\Develop\Misc Workspace\archive\pinfeather-core-prototype\runtime-data' (Get-Date -Format 'yyyyMMdd-HHmmss'))
)

$ErrorActionPreference = 'Stop'

# 1) 在规范化、建目录、启动 npm 之前，校验原始 DataDir 为完整绝对路径。
#    完整绝对路径 = 盘符+\（如 C:\data）或 UNC（\\server\share）。拒绝空值、相对路径、
#    驱动相对（C:foo）、根相对（\foo）。
if ([string]::IsNullOrWhiteSpace($DataDir)) {
    Write-Host 'DataDir 不能为空。用法：powershell -File tools\dev.ps1 -DataDir <完整绝对路径>'
    exit 1
}
if (-not ($DataDir -match '^[A-Za-z]:[\\/]' -or $DataDir -match '^\\\\')) {
    Write-Host "DataDir 不是完整绝对路径（拒绝空值、相对路径、C:foo、\foo）：$DataDir"
    exit 1
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repoRoot

$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH

$listener = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($listener) {
    $listener | ForEach-Object {
        $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue
        Write-Host ("端口 1420 已被占用：PID {0} ({1})。请先自行处理，本脚本不会结束其他进程。" -f $_.OwningProcess, $(if ($p) { $p.ProcessName } else { '未知进程' }))
    }
    exit 1
}

# 2) 校验通过后才规范化、创建目录并启动 npm
$resolved = [System.IO.Path]::GetFullPath($DataDir)
New-Item -ItemType Directory -Force -Path $resolved | Out-Null
Write-Host "数据目录：$resolved"
Write-Host "启动参数：--data-dir $resolved --start-paused"

# tauri dev 参数语义：第一个 -- 之后是 runner(cargo) 参数，第二个 -- 之后才是应用参数
& npm run tauri -- dev -- -- --data-dir $resolved --start-paused
exit $LASTEXITCODE
