param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [switch]$ResumeCapture
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
if ([IO.Path]::GetExtension($exe) -ne '.exe') { throw '请选择片羽 exe 文件。' }
$launchArgs = @('--data-dir', 'D:\Develop\PinfeatherData\personal')
if (-not $ResumeCapture) { $launchArgs += '--start-paused' }
Start-Process -FilePath $exe -ArgumentList $launchArgs -WindowStyle Hidden
