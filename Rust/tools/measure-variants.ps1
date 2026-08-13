[CmdletBinding()]
param(
    [switch]$Benchmark,
    [switch]$Profiling
)

if ($Benchmark -and $Profiling) {
    throw "Choose only one of -Benchmark or -Profiling."
}

$ErrorActionPreference = "Stop"
$rustRoot = Split-Path -Parent $PSScriptRoot
$repositoryRoot = Split-Path -Parent $rustRoot
$llvmSize = Get-ChildItem (Join-Path (rustc --print sysroot) "lib\rustlib") `
    -Recurse -Filter llvm-size.exe -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName

function Measure-CodeLines([string[]]$RelativePaths) {
    $count = 0
    foreach ($relativePath in $RelativePaths) {
        $path = Join-Path $rustRoot $relativePath
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Trade-study source file not found: '$path'"
        }
        $count += @(Get-Content -LiteralPath $path | Where-Object {
            $trimmed = $_.Trim()
            $trimmed -ne "" -and -not $trimmed.StartsWith("//")
        }).Count
    }
    $count
}

function Measure-CCodeLines([string[]]$RelativePaths) {
    $count = 0
    foreach ($relativePath in $RelativePaths) {
        $path = Join-Path $repositoryRoot $relativePath
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Trade-study source file not found: '$path'"
        }

        $insideBlockComment = $false
        foreach ($line in Get-Content -LiteralPath $path) {
            $trimmed = $line.Trim()
            if ($insideBlockComment) {
                if ($trimmed.Contains("*/")) { $insideBlockComment = $false }
                continue
            }
            if ($trimmed.StartsWith("/*")) {
                if (-not $trimmed.Contains("*/")) { $insideBlockComment = $true }
                continue
            }
            if ($trimmed -and -not $trimmed.StartsWith("//")) { $count++ }
        }
    }
    $count
}

function Measure-Elf([string]$Name) {
    $path = Join-Path $rustRoot "target\thumbv7em-none-eabihf\release\$Name"
    if (-not $llvmSize -or -not (Test-Path -LiteralPath $path)) {
        return $null
    }

    $sections = @{}
    & $llvmSize -A $path | ForEach-Object {
        if ($_ -match '^\.(\S+)\s+(\d+)\s+') {
            $sections[$matches[1]] = [int64]$matches[2]
        }
    }
    [pscustomobject]@{
        FlashBytes = $sections.vector_table + $sections.text + $sections.rodata + $sections.data
        McuRamBytes = $sections.data + $sections.bss + $sections.uninit
    }
}

function Measure-CElf {
    $path = Join-Path $repositoryRoot "STM32CubeIDE\build\release\LwIP_UDP_Echo_Server.elf"
    $sizeTool = Get-ChildItem "$env:APPDATA\xPacks\@xpack-dev-tools\arm-none-eabi-gcc" `
        -Recurse -Filter arm-none-eabi-size.exe -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $sizeTool -or -not (Test-Path -LiteralPath $path)) { return $null }

    $columns = ((& $sizeTool $path | Select-Object -Last 1).Trim() -split "\s+")
    [pscustomobject]@{
        FlashBytes = [int64]$columns[0] + [int64]$columns[1]
        McuRamBytes = [int64]$columns[1] + [int64]$columns[2]
    }
}

# Board clock/MCUboot setup is shared platform code and intentionally excluded.
# Each bring-up set includes the variant's thin integration binary because it
# constructs the IP stack or coordinates DHCP with the selected UDP server.
$variants = @(
    [pscustomobject]@{
        Variant = "Native RMII + Embassy"
        BringUp = @(
            "src/bin/native_rmii_udp_echo.rs",
            "src/bringup/native_rmii.rs",
            "src/bringup/embassy_network.rs"
        )
        Server = @("src/servers/embassy_udp_echo.rs")
        Artifact = "artifacts/firmware-native-udp-signed.bin"
        BenchmarkArtifact = "artifacts/firmware-native-udp-benchmark-signed.bin"
        ProfilingArtifact = "artifacts/firmware-native-udp-profiling-signed.bin"
        Elf = "nucleo-h723zg-native-rmii-udp-echo"
    },
    [pscustomobject]@{
        Variant = "W5500 MACRAW + Embassy"
        BringUp = @(
            "src/bin/w5500_macraw_udp_echo.rs",
            "src/bringup/w5500_spi.rs",
            "src/bringup/w5500_macraw.rs",
            "src/bringup/embassy_network.rs"
        )
        Server = @("src/servers/embassy_udp_echo.rs")
        Artifact = "artifacts/firmware-w5500-signed.bin"
        BenchmarkArtifact = "artifacts/firmware-w5500-benchmark-signed.bin"
        ProfilingArtifact = "artifacts/firmware-w5500-profiling-signed.bin"
        Elf = "nucleo-h723zg-w5500-udp-echo"
    },
    [pscustomobject]@{
        Variant = "W5500 hardware offload"
        BringUp = @(
            "src/bin/w5500_offload_udp_echo.rs",
            "src/bringup/w5500_spi.rs",
            "src/bringup/w5500_offload.rs"
        )
        Server = @("src/servers/w5500_offload_udp_echo.rs")
        Artifact = "artifacts/firmware-w5500-offload-signed.bin"
        BenchmarkArtifact = "artifacts/firmware-w5500-offload-benchmark-signed.bin"
        ProfilingArtifact = "artifacts/firmware-w5500-offload-profiling-signed.bin"
        Elf = "nucleo-h723zg-w5500-offload-udp-echo"
    }
)

$cElf = Measure-CElf
$results = @([pscustomobject]@{
    Variant = "C/LwIP native RMII"
    BringUpNCLOC = Measure-CCodeLines @("Src/main.c", "Src/app_ethernet.c", "Src/ethernetif.c")
    UdpServerNCLOC = Measure-CCodeLines @("Src/udp_echoserver.c")
    StudyTotalNCLOC = 0
    SignedBytes = $null
    ElfFlashBytes = $cElf.FlashBytes
    McuRamBytes = $cElf.McuRamBytes
})
$results[0].StudyTotalNCLOC = $results[0].BringUpNCLOC + $results[0].UdpServerNCLOC

$results += foreach ($variant in $variants) {
    $bringUpLines = Measure-CodeLines $variant.BringUp
    $serverLines = Measure-CodeLines $variant.Server
    $artifact = if ($Profiling) {
        $variant.ProfilingArtifact
    } elseif ($Benchmark) {
        $variant.BenchmarkArtifact
    } else {
        $variant.Artifact
    }
    $artifactPath = Join-Path $rustRoot $artifact
    $signedBytes = if (Test-Path -LiteralPath $artifactPath) {
        (Get-Item -LiteralPath $artifactPath).Length
    } else {
        $null
    }
    $elf = Measure-Elf $variant.Elf

    [pscustomobject]@{
        Variant = $variant.Variant
        BringUpNCLOC = $bringUpLines
        UdpServerNCLOC = $serverLines
        StudyTotalNCLOC = $bringUpLines + $serverLines
        SignedBytes = $signedBytes
        ElfFlashBytes = $elf.FlashBytes
        McuRamBytes = $elf.McuRamBytes
    }
}

$results | Format-Table -AutoSize
Write-Host ""
Write-Host "NCLOC excludes blank lines and full-line // comments; braces and declarations count."
Write-Host "Shared files count in every variant that must maintain and compile them."
Write-Host "Shared board.rs, tests, tools, bootloader, SSH, and update code are outside this scope."
Write-Host "C bring-up includes main.c because its clock, MPU, loop, and netif integration are generated as one unit."
Write-Host "ELF flash/RAM columns describe the latest release ELF; build all variants with matching features first."
if (-not $llvmSize) {
    Write-Warning "llvm-size is unavailable. Install it with: rustup component add llvm-tools-preview"
}
