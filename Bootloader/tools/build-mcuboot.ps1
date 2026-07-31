[CmdletBinding()]
param(
    [string]$ZephyrWorkspace = (Join-Path $env:USERPROFILE "zephyrproject-v4.4.0"),
    [switch]$Pristine
)

$ErrorActionPreference = "Stop"
$bootloaderRoot = Split-Path -Parent $PSScriptRoot
$buildDirectory = Join-Path $bootloaderRoot "build"
$westPython = Join-Path $ZephyrWorkspace ".venv313\Scripts\python.exe"
$mcubootSource = Join-Path $ZephyrWorkspace "bootloader\mcuboot\boot\zephyr"
$signingKey = Join-Path $bootloaderRoot "root-ed25519.pem"

foreach ($requiredFile in @($westPython, $mcubootSource, $signingKey)) {
    if (-not (Test-Path -LiteralPath $requiredFile)) {
        throw "Required path not found: '$requiredFile'"
    }
}

# CMake and Ninja are installed inside this virtual environment by the setup
# procedure, so west and all of its subprocesses see the same pinned tools.
$venvScripts = Join-Path $ZephyrWorkspace ".venv313\Scripts"
$env:PATH = "$venvScripts;$env:PATH"

$westArguments = @("build")
if ($Pristine) {
    $westArguments += @("-p", "always")
}
$westArguments += @(
    "-b", "nucleo_h723zg",
    $mcubootSource,
    "-d", $buildDirectory,
    "--",
    "-DAPPLICATION_CONFIG_DIR=$($bootloaderRoot.Replace('\', '/'))"
)

Push-Location $ZephyrWorkspace
try {
    & $westPython -m west @westArguments
    if ($LASTEXITCODE -ne 0) {
        throw "MCUboot build failed."
    }
}
finally {
    Pop-Location
}

$elf = Join-Path $buildDirectory "zephyr\zephyr.elf"
$hex = Join-Path $buildDirectory "zephyr\zephyr.hex"
Write-Host "MCUboot build complete:"
Write-Host "  ELF: $elf"
Write-Host "  HEX: $hex"
