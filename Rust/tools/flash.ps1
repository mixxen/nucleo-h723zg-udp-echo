[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [ValidatePattern("^\d+\.\d+\.\d+(\+\d+)?$")]
    [string]$Version = "0.1.0",
    [string]$ZephyrWorkspace = (Join-Path $env:USERPROFILE "zephyrproject-v4.4.0"),
    [switch]$W5500,
    [switch]$W5500Offload
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Split-Path -Parent $projectRoot

if ($W5500 -and $W5500Offload) {
    throw "Choose either -W5500 or -W5500Offload, not both."
}

if (-not $SkipBuild) {
    & (Join-Path $repositoryRoot "Bootloader\tools\build-mcuboot.ps1") `
        -ZephyrWorkspace $ZephyrWorkspace
    if ($LASTEXITCODE -ne 0) {
        throw "MCUboot build failed."
    }

    & (Join-Path $PSScriptRoot "build-signed.ps1") `
        -Version $Version `
        -ZephyrWorkspace $ZephyrWorkspace `
        -W5500:$W5500 `
        -W5500Offload:$W5500Offload
    if ($LASTEXITCODE -ne 0) {
        throw "Signed application build failed."
    }
}

$bootloaderHex = Join-Path $repositoryRoot "Bootloader\build\zephyr\zephyr.hex"
$signedApplicationName = if ($W5500Offload) {
    "firmware-w5500-offload-signed.bin"
} elseif ($W5500) {
    "firmware-w5500-signed.bin"
} else {
    "firmware-signed.bin"
}
$signedApplication = Join-Path $projectRoot "artifacts\$signedApplicationName"
foreach ($requiredFile in @($bootloaderHex, $signedApplication)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Factory image component not found: '$requiredFile'"
    }
}

$openOcdRoot = Join-Path $env:APPDATA "xPacks\@xpack-dev-tools\openocd"
$openOcd = Get-ChildItem -LiteralPath $openOcdRoot -Recurse -Filter openocd.exe -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $openOcd) {
    throw "OpenOCD was not found below '$openOcdRoot'. Install @xpack-dev-tools/openocd first."
}

$bootloaderForOpenOcd = (Resolve-Path -LiteralPath $bootloaderHex).Path.Replace("\", "/")
$applicationForOpenOcd = (Resolve-Path -LiteralPath $signedApplication).Path.Replace("\", "/")

# A factory install intentionally erases all internal flash so stale slot
# trailers cannot request an unexpected swap. Subsequent releases use SSH and
# touch only the secondary slot.
$commands = @(
    "init",
    "reset halt",
    "stm32h7x mass_erase 0",
    "flash write_image {$bootloaderForOpenOcd}",
    "verify_image {$bootloaderForOpenOcd}",
    "flash write_image {$applicationForOpenOcd} 0x08020000 bin",
    "verify_image {$applicationForOpenOcd} 0x08020000 bin",
    "reset run",
    "shutdown"
) -join "; "

& $openOcd `
    -f interface/stlink.cfg `
    -f target/stm32h7x.cfg `
    -c "adapter speed 3300" `
    -c $commands

if ($LASTEXITCODE -ne 0) {
    throw "OpenOCD failed with exit code $LASTEXITCODE"
}

Write-Host "MCUboot and signed Rust firmware programmed, verified, and started."
