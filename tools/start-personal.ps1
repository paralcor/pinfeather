param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
if ([IO.Path]::GetExtension($exe) -ne '.exe') { throw '请选择片羽 exe 文件。' }
Start-Process -FilePath $exe -ArgumentList @('--data-dir', 'D:\Develop\PinfeatherData\personal', '--start-paused') -WindowStyle Hidden
