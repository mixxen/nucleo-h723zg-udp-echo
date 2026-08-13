[CmdletBinding()]
param(
    [ValidatePattern("^\d+\.\d+\.\d+(\+\d+)?$")]
    [string]$Version = "0.1.0",
    [string]$ZephyrWorkspace = (Join-Path $env:USERPROFILE "zephyrproject-v4.4.0"),
    [string]$ZephyrSdk = (Join-Path $env:USERPROFILE "zephyr-sdk-1.0.1"),
    [switch]$RollbackTest,
    [switch]$NativeUdp,
    [switch]$W5500,
    [switch]$W5500Offload,
    [switch]$Benchmark,
    [switch]$Profiling,
    [switch]$Performance
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Split-Path -Parent $projectRoot
$artifacts = Join-Path $projectRoot "artifacts"
$privateKey = Join-Path $repositoryRoot "Bootloader\root-ed25519.pem"
$imgtool = Join-Path $ZephyrWorkspace ".venv313\Scripts\imgtool.exe"
$objcopy = Join-Path $ZephyrSdk "gnu\arm-zephyr-eabi\bin\arm-zephyr-eabi-objcopy.exe"
$variantCount = @($NativeUdp, $W5500, $W5500Offload).Where({ $_ }).Count
if ($variantCount -gt 1) {
    throw "Choose only one of -NativeUdp, -W5500, or -W5500Offload."
}
if (($NativeUdp -or $W5500 -or $W5500Offload) -and $RollbackTest) {
    throw "-RollbackTest applies only to the managed native Ethernet firmware."
}
if ($Benchmark -and -not ($NativeUdp -or $W5500 -or $W5500Offload)) {
    throw "-Benchmark requires -NativeUdp, -W5500, or -W5500Offload."
}
if ($Profiling -and -not ($NativeUdp -or $W5500 -or $W5500Offload)) {
    throw "-Profiling requires -NativeUdp, -W5500, or -W5500Offload."
}
if ($Performance -and -not ($NativeUdp -or $W5500Offload)) {
    throw "-Performance requires -NativeUdp or -W5500Offload."
}
if ($Performance -and ($Profiling -or $Benchmark)) {
    throw "-Performance already disables packet logging; do not combine it with -Benchmark or -Profiling."
}

$binaryName = if ($W5500Offload) {
    "nucleo-h723zg-w5500-offload-udp-echo"
} elseif ($W5500) {
    "nucleo-h723zg-w5500-udp-echo"
} elseif ($NativeUdp) {
    "nucleo-h723zg-native-rmii-udp-echo"
} else {
    "nucleo-h723zg-udp-echo"
}
$artifactPrefix = if ($W5500Offload) { "firmware-w5500-offload" } elseif ($W5500) { "firmware-w5500" } elseif ($NativeUdp) { "firmware-native-udp" } else { "firmware" }
if ($Profiling) {
    $artifactPrefix += "-profiling"
} elseif ($Performance) {
    $artifactPrefix += "-performance"
} elseif ($Benchmark) {
    $artifactPrefix += "-benchmark"
}
$cargoProfile = if ($Performance) { "performance" } else { "release" }
$elf = Join-Path $projectRoot "target\thumbv7em-none-eabihf\$cargoProfile\$binaryName"
$unsignedBinary = Join-Path $artifacts "$artifactPrefix-unsigned.bin"
$signedBinary = Join-Path $artifacts "$artifactPrefix-signed.bin"

foreach ($requiredFile in @($privateKey, $imgtool, $objcopy)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Required file not found: '$requiredFile'"
    }
}

New-Item -ItemType Directory -Path $artifacts -Force | Out-Null

Push-Location $projectRoot
try {
    $cargoArguments = @("build", "--locked", "--profile", $cargoProfile)
    if ($NativeUdp -or $W5500 -or $W5500Offload) {
        $feature = if ($W5500Offload) { "wiznet-offload" } elseif ($W5500) { "wiznet" } else { "native-udp" }
        if ($Profiling) {
            $feature += ",profiling"
        } elseif ($Performance) {
            $feature += ",performance"
        } elseif ($Benchmark) {
            $feature += ",benchmark"
        }
        $cargoArguments += @(
            "--no-default-features",
            "--features", $feature,
            "--bin", $binaryName
        )
    }
    elseif ($RollbackTest) {
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
