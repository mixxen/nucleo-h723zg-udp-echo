[CmdletBinding()]
param(
    [ValidatePattern("^\d+\.\d+\.\d+(\+\d+)?$")]
    [string]$Version = "0.4.0",
    [string]$NativeIp = "192.168.68.66",
    [string]$W5500Ip = "192.168.68.74",
    [string]$OutputRoot,
    [switch]$SkipFirmwareBuild,
    [switch]$Quick
)

$ErrorActionPreference = "Stop"
$rustRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Split-Path -Parent $rustRoot
$benchmarkRoot = Join-Path $PSScriptRoot "udp-benchmark"
$hostTarget = "x86_64-pc-windows-msvc"
$benchmarkExe = Join-Path $benchmarkRoot "target\$hostTarget\release\udp-benchmark.exe"

if (-not $OutputRoot) {
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputRoot = Join-Path $rustRoot "benchmark-results\$timestamp"
}
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
New-Item -ItemType Directory -Path $OutputRoot -Force | Out-Null

if ($Quick) {
    $latencySamples = 200
    $throughputSeconds = 2
    $burstRepetitions = 10
    $soakSeconds = 60
    $soakIntervalSeconds = 20
}
else {
    $latencySamples = 10000
    $throughputSeconds = 10
    $burstRepetitions = 100
    $soakSeconds = 900
    $soakIntervalSeconds = 60
}

Write-Host "Building and testing the Windows benchmark tool..."
& cargo test `
    --locked `
    --manifest-path (Join-Path $benchmarkRoot "Cargo.toml") `
    --target $hostTarget
if ($LASTEXITCODE -ne 0) {
    throw "Benchmark tool tests failed."
}
& cargo build `
    --locked `
    --release `
    --manifest-path (Join-Path $benchmarkRoot "Cargo.toml") `
    --target $hostTarget
if ($LASTEXITCODE -ne 0) {
    throw "Benchmark tool release build failed."
}

$variants = @(
    [pscustomobject]@{
        Name = "Native RMII + Embassy"
        Slug = "native-rmii"
        Ip = $NativeIp
        Mac = "02-00-00-00-00-00"
        BuildSwitch = "NativeUdp"
        SpiHz = $null
        NoZeroByte = $false
    },
    [pscustomobject]@{
        Name = "W5500 MACRAW + Embassy"
        Slug = "w5500-macraw"
        Ip = $W5500Ip
        Mac = "02-00-00-00-55-00"
        BuildSwitch = "W5500"
        SpiHz = 20000000
        NoZeroByte = $false
    },
    [pscustomobject]@{
        Name = "W5500 hardware offload"
        Slug = "w5500-offload"
        Ip = $W5500Ip
        Mac = "02-00-00-00-55-00"
        BuildSwitch = "W5500Offload"
        SpiHz = 20000000
        NoZeroByte = $true
    }
)

if (-not $SkipFirmwareBuild) {
    foreach ($variant in $variants) {
        Write-Host "Building signed benchmark firmware: $($variant.Name)"
        $buildArguments = @{
            Version = $Version
            Benchmark = $true
        }
        $buildArguments[$variant.BuildSwitch] = $true
        & (Join-Path $PSScriptRoot "build-signed.ps1") @buildArguments
        if ($LASTEXITCODE -ne 0) {
            throw "Firmware build failed for $($variant.Name)."
        }
    }
}

$suiteFiles = @()
foreach ($variant in $variants) {
    Write-Host "Flashing benchmark firmware: $($variant.Name)"
    $flashArguments = @{
        SkipBuild = $true
        Benchmark = $true
    }
    $flashArguments[$variant.BuildSwitch] = $true
    & (Join-Path $PSScriptRoot "flash.ps1") @flashArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Firmware flash failed for $($variant.Name)."
    }

    $functionalSizes = if ($variant.NoZeroByte) { "1,32,256,1472" } else { "0,1,32,256,1472" }
    $ready = $false
    for ($attempt = 1; $attempt -le 30; $attempt++) {
        Start-Sleep -Seconds 1
        try {
            & (Join-Path $PSScriptRoot "udp_echo_test.ps1") `
                -BoardIp $variant.Ip `
                -Sizes $functionalSizes `
                -Count 1 *> $null
            if ($LASTEXITCODE -eq 0) {
                $ready = $true
                break
            }
        }
        catch {
            # DHCP and link negotiation are still in progress. Retry below.
        }
    }
    if (-not $ready) {
        throw "$($variant.Name) did not answer its functional gate at $($variant.Ip)."
    }

    $variantOutput = Join-Path $OutputRoot $variant.Slug
    $suiteArguments = @(
        "suite",
        "--board", $variant.Ip,
        "--variant", $variant.Name,
        "--mac", $variant.Mac,
        "--firmware-version", $Version,
        "--latency-samples", $latencySamples,
        "--throughput-seconds", $throughputSeconds,
        "--burst-repetitions", $burstRepetitions,
        "--soak-seconds", $soakSeconds,
        "--soak-interval-seconds", $soakIntervalSeconds,
        "--output-dir", $variantOutput
    )
    if ($variant.SpiHz) {
        $suiteArguments += @("--spi-hz", $variant.SpiHz)
    }
    if ($variant.NoZeroByte) {
        $suiteArguments += "--no-zero-byte"
    }

    Write-Host "Running benchmark suite: $($variant.Name)"
    & $benchmarkExe @suiteArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Benchmark suite failed for $($variant.Name)."
    }
    $suiteFiles += Join-Path $variantOutput "suite.json"
}

$generatedReport = Join-Path $OutputRoot "BENCHMARK_REPORT.md"
& $benchmarkExe compare --inputs @suiteFiles --output $generatedReport
if ($LASTEXITCODE -ne 0) {
    throw "Benchmark report generation failed."
}

Write-Host "Benchmark comparison complete:"
Write-Host "  Raw results: $OutputRoot"
Write-Host "  Report: $(Join-Path $rustRoot 'BENCHMARK_REPORT.md')"
Write-Host "  Generated result tables: $generatedReport"
Write-Host "Review the generated tables before updating the curated Rust\BENCHMARK_REPORT.md."
