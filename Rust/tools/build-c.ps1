[CmdletBinding()]
param(
    [string]$CubeRoot,
    [ValidateSet("O2", "O3", "Os")]
    [string]$Optimization = "O2"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$workspaceParent = Split-Path -Parent $repositoryRoot
if (-not $CubeRoot) { $CubeRoot = Join-Path $workspaceParent "STM32CubeH7" }
$project = Join-Path $repositoryRoot "STM32CubeIDE"
$makefile = Join-Path $project "STM32Make.make"
$cubeRootPath = [IO.Path]::GetFullPath($CubeRoot).Replace("\", "/")

foreach ($required in @($makefile, (Join-Path $CubeRoot "Drivers"), (Join-Path $CubeRoot "Middlewares\Third_Party\LwIP"))) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required C build input not found: '$required'"
    }
}
if (-not (Get-Command make.exe -ErrorAction SilentlyContinue)) {
    throw "make.exe was not found on PATH. Install the STM32 for VS Code build tools first."
}

& make.exe -f $makefile -j ([Environment]::ProcessorCount) DEBUG=0 "OPTIMIZATION=-$Optimization" "CUBE_ROOT=$cubeRootPath" -C $project
if ($LASTEXITCODE -ne 0) {
    throw "C/LwIP release build failed."
}

$elf = Join-Path $project "build\release\LwIP_UDP_Echo_Server.elf"
if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) {
    throw "C/LwIP release ELF was not created: '$elf'"
}
Write-Host "C/LwIP release build complete: $elf"
