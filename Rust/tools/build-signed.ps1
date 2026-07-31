[CmdletBinding()]
param(
    [ValidatePattern("^\d+\.\d+\.\d+(\+\d+)?$")]
    [string]$Version = "0.1.0",
    [string]$ZephyrWorkspace = (Join-Path $env:USERPROFILE "zephyrproject-v4.4.0"),
    [string]$ZephyrSdk = (Join-Path $env:USERPROFILE "zephyr-sdk-1.0.1"),
    [switch]$RollbackTest
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Split-Path -Parent $projectRoot
$artifacts = Join-Path $projectRoot "artifacts"
$privateKey = Join-Path $repositoryRoot "Bootloader\root-ed25519.pem"
$imgtool = Join-Path $ZephyrWorkspace ".venv313\Scripts\imgtool.exe"
$objcopy = Join-Path $ZephyrSdk "gnu\arm-zephyr-eabi\bin\arm-zephyr-eabi-objcopy.exe"
$elf = Join-Path $projectRoot "target\thumbv7em-none-eabihf\release\nucleo-h723zg-udp-echo"
$unsignedBinary = Join-Path $artifacts "firmware-unsigned.bin"
$signedBinary = Join-Path $artifacts "firmware-signed.bin"

foreach ($requiredFile in @($privateKey, $imgtool, $objcopy)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Required file not found: '$requiredFile'"
    }
}

New-Item -ItemType Directory -Path $artifacts -Force | Out-Null

Push-Location $projectRoot
try {
    $cargoArguments = @("build", "--locked", "--release")
    if ($RollbackTest) {
        Write-Warning "Building a hardware-test image that will NOT confirm itself."
        $cargoArguments += @("--features", "rollback-test")
    }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo release build failed."
    }
}
finally {
    Pop-Location
}

& $objcopy -O binary $elf $unsignedBinary
if ($LASTEXITCODE -ne 0) {
    throw "objcopy failed to create the unsigned application binary."
}

# The binary starts at the Rust vector table. imgtool prepends the reserved
# 0x200-byte header, signs the complete image, and rejects slot overflow.
& $imgtool sign `
    --key $privateKey `
    --align 32 `
    --max-align 32 `
    --header-size 0x200 `
    --pad-header `
    --slot-size 0x60000 `
    --max-sectors 4 `
    --version $Version `
    $unsignedBinary `
    $signedBinary
if ($LASTEXITCODE -ne 0) {
    throw "imgtool failed to sign the application."
}

& $imgtool verify -k $privateKey $signedBinary
if ($LASTEXITCODE -ne 0) {
    throw "The generated application signature did not verify."
}

$signedSize = (Get-Item -LiteralPath $signedBinary).Length
if ($signedSize -gt 0x40000) {
    throw "Signed image is $signedSize bytes; offset swap allows at most 262144 bytes before its trailer sector."
}
Write-Host "Signed application created:"
Write-Host "  $signedBinary"
Write-Host "  Version: $Version"
Write-Host "  Size: $signedSize bytes of the 262144-byte effective image area"
