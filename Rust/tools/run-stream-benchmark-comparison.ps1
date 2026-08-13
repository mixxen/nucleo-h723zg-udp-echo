[CmdletBinding()]
param(
    [ValidatePattern("^\d+\.\d+\.\d+(\+\d+)?$")]
    [string]$Version = "0.4.2",
    [Parameter(Mandatory)]
    [string]$NativeIp,
    [string]$CNativeIp,
    [Parameter(Mandatory)]
    [string]$W5500Ip,
    [string]$CubeRoot,
    [string]$OutputRoot,
    [switch]$SkipFirmwareBuild,
    [switch]$Quick
)

$ErrorActionPreference = "Stop"
if (-not $CNativeIp) { $CNativeIp = $NativeIp }
$rustRoot = Split-Path -Parent $PSScriptRoot
$benchmarkRoot = Join-Path $PSScriptRoot "udp-benchmark"
$hostTarget = "x86_64-pc-windows-msvc"
$benchmarkExe = Join-Path $benchmarkRoot "target\$hostTarget\release\udp-benchmark.exe"

function Get-CStaticRamBytes {
    $elf = Join-Path (Split-Path -Parent $rustRoot) "STM32CubeIDE\build\release\LwIP_UDP_Echo_Server.elf"
    $sizeTool = Get-ChildItem "$env:APPDATA\xPacks\@xpack-dev-tools\arm-none-eabi-gcc" `
        -Recurse -Filter "arm-none-eabi-size.exe" | Select-Object -First 1 -ExpandProperty FullName
    if (-not $sizeTool -or -not (Test-Path $elf)) { return $null }

    $columns = ((& $sizeTool $elf | Select-Object -Last 1).Trim() -split "\s+")
    if ($LASTEXITCODE -ne 0 -or $columns.Count -lt 3) { return $null }
    return [int64]$columns[1] + [int64]$columns[2]
}

if (-not $OutputRoot) {
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputRoot = Join-Path $rustRoot "benchmark-results\stream-$timestamp"
}
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
New-Item -ItemType Directory -Path $OutputRoot -Force | Out-Null
$duration = if ($Quick) { 30 } else { 3600 }
$interval = if ($Quick) { 10 } else { 60 }
$sweepDuration = if ($Quick) { 3 } else { 30 }

& cargo test --locked --manifest-path (Join-Path $benchmarkRoot "Cargo.toml") --target $hostTarget
if ($LASTEXITCODE -ne 0) { throw "Benchmark tool tests failed." }
& cargo build --locked --release --manifest-path (Join-Path $benchmarkRoot "Cargo.toml") --target $hostTarget
if ($LASTEXITCODE -ne 0) { throw "Benchmark tool build failed." }

$variants = @(
    [pscustomobject]@{ Name = "C/LwIP native RMII"; Slug = "c-lwip-native-rmii"; Ip = $CNativeIp; BuildSwitch = $null; Profile = $false },
    [pscustomobject]@{ Name = "Rust native RMII + Embassy"; Slug = "native-rmii"; Ip = $NativeIp; BuildSwitch = "NativeUdp"; Profile = $true },
    [pscustomobject]@{ Name = "Rust W5500 MACRAW + Embassy"; Slug = "w5500-macraw"; Ip = $W5500Ip; BuildSwitch = "W5500"; Profile = $true },
    [pscustomobject]@{ Name = "Rust W5500 hardware offload"; Slug = "w5500-offload"; Ip = $W5500Ip; BuildSwitch = "W5500Offload"; Profile = $true }
)

$rows = @()
$rateRows = @()
foreach ($variant in $variants) {
    if (-not $SkipFirmwareBuild) {
        if ($variant.Profile) {
            $build = @{ Version = $Version; Profiling = $true }
            $build[$variant.BuildSwitch] = $true
            & (Join-Path $PSScriptRoot "build-signed.ps1") @build
        } else {
            $build = @{}
            if ($CubeRoot) { $build.CubeRoot = $CubeRoot }
            & (Join-Path $PSScriptRoot "build-c.ps1") @build
        }
        if ($LASTEXITCODE -ne 0) { throw "Firmware build failed for $($variant.Name)." }
    }

    if ($variant.Profile) {
        $flash = @{ SkipBuild = $true; Profiling = $true }
        $flash[$variant.BuildSwitch] = $true
        & (Join-Path $PSScriptRoot "flash.ps1") @flash
    } else {
        & (Join-Path $PSScriptRoot "flash-c.ps1") -SkipBuild
    }
    if ($LASTEXITCODE -ne 0) { throw "Firmware flash failed for $($variant.Name)." }
    $cStaticRamBytes = if ($variant.Profile) { $null } else { Get-CStaticRamBytes }

    $ready = $false
    for ($attempt = 1; $attempt -le 30; $attempt++) {
        Start-Sleep -Seconds 1
        try {
            & (Join-Path $PSScriptRoot "udp_echo_test.ps1") -BoardIp $variant.Ip -Sizes "100" -Count 1 *> $null
            if ($LASTEXITCODE -eq 0) { $ready = $true; break }
        } catch { }
    }
    if (-not $ready) { throw "$($variant.Name) did not answer at $($variant.Ip). Check its DHCP lease." }

    $output = Join-Path $OutputRoot $variant.Slug
    $profileArgument = if ($variant.Profile) { @("--profile") } else { @() }
    & $benchmarkExe stream --board $variant.Ip --payload-bytes 100 --rate-hz 1000 `
        --duration-seconds $duration --interval-seconds $interval @profileArgument --output-dir $output
    if ($LASTEXITCODE -ne 0) { throw "Stream benchmark failed for $($variant.Name)." }

    $sweepOutput = Join-Path $output "rate-sweep"
    & $benchmarkExe stream-sweep --board $variant.Ip --payload-bytes 100 `
        --duration-seconds $sweepDuration @profileArgument --output-dir $sweepOutput
    if ($LASTEXITCODE -ne 0) { throw "Stream rate sweep failed for $($variant.Name)." }

    $stream = Get-Content (Join-Path $output "stream.json") | ConvertFrom-Json
    $result = $stream.result
    $sweep = Get-Content (Join-Path $sweepOutput "stream-sweep.json") | ConvertFrom-Json
    foreach ($point in $sweep.points) {
        $rateRows += [pscustomobject]@{
            Variant = $variant.Name
            TargetKHz = $point.target_hz / 1000
            AchievedKHz = $point.achieved_hz / 1000
            Reliable = $point.reliable
            ErrorEvents = $point.error_events
            Missing = $point.result.counters.missing
            Late = $point.result.counters.late
            Duplicate = $point.result.counters.duplicates
            Reordered = $point.result.counters.reordered
            Corrupt = $point.result.counters.corrupt
            Foreign = $point.result.counters.foreign
            SendErrors = $point.result.counters.send_errors
            ExecutorCpuPercent = $point.profile.executor_cpu_percent
            CyclesPerValid = $point.profile.cycles_per_valid_packet
            StackHighWaterBytes = $point.profile.stack_high_water_bytes
            StaticRamBytes = if ($variant.Profile) { $point.profile.static_ram_bytes } else { $cStaticRamBytes }
        }
    }
    $rows += [pscustomobject]@{
        Variant = $variant.Name
        Sent = $result.counters.sent
        Valid = $result.counters.valid_replies
        Missing = $result.counters.missing
        Late = $result.counters.late
        Duplicate = $result.counters.duplicates
        Reordered = $result.counters.reordered
        Corrupt = $result.counters.corrupt
        P50ms = $result.latency.p50_ns / 1e6
        P99ms = $result.latency.p99_ns / 1e6
        MaximumMs = $result.latency.max_ns / 1e6
        ExecutorCpuPercent = $stream.profile.executor_cpu_percent
        CyclesPerValid = $stream.profile.cycles_per_valid_packet
        StackHighWaterBytes = $stream.profile.stack_high_water_bytes
        StaticRamBytes = if ($variant.Profile) { $stream.profile.static_ram_bytes } else { $cStaticRamBytes }
        HighestReliableKHz = if ($null -eq $sweep.highest_reliable_hz) { $null } else { $sweep.highest_reliable_hz / 1000 }
        FirstUnreliableKHz = if ($null -eq $sweep.first_unreliable_hz) { $null } else { $sweep.first_unreliable_hz / 1000 }
    }
}

$rows | Format-Table -AutoSize
$rateRows | Format-Table -AutoSize
$rows | ConvertTo-Json | Set-Content (Join-Path $OutputRoot "comparison.json")
$rateRows | Export-Csv -NoTypeInformation (Join-Path $OutputRoot "rate-comparison.csv")
Write-Host "Stream comparison complete: $OutputRoot"
