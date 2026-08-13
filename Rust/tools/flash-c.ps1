[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$CubeRoot
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$elf = Join-Path $repositoryRoot "STM32CubeIDE\build\release\LwIP_UDP_Echo_Server.elf"

if (-not $SkipBuild) {
    $arguments = @{}
    if ($CubeRoot) { $arguments.CubeRoot = $CubeRoot }
    & (Join-Path $PSScriptRoot "build-c.ps1") @arguments
    if ($LASTEXITCODE -ne 0) { throw "C/LwIP build failed." }
}
if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) {
    throw "C/LwIP release ELF not found: '$elf'"
}

$openOcdRoot = Join-Path $env:APPDATA "xPacks\@xpack-dev-tools\openocd"
$openOcd = Get-ChildItem -LiteralPath $openOcdRoot -Recurse -Filter openocd.exe -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $openOcd) {
    throw "OpenOCD was not found below '$openOcdRoot'."
}

$elfForOpenOcd = (Resolve-Path -LiteralPath $elf).Path.Replace("\", "/")
& $openOcd -f interface/stlink.cfg -f target/stm32h7x.cfg `
    -c "init; reset halt; program {$elfForOpenOcd} verify reset; shutdown"
if ($LASTEXITCODE -ne 0) { throw "C/LwIP flash or verification failed." }
Write-Host "C/LwIP native-RMII firmware programmed, verified, and started."
