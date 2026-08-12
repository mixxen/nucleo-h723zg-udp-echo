[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$rustRoot = Split-Path -Parent $PSScriptRoot

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
    }
)

$results = foreach ($variant in $variants) {
    $bringUpLines = Measure-CodeLines $variant.BringUp
    $serverLines = Measure-CodeLines $variant.Server
    $artifactPath = Join-Path $rustRoot $variant.Artifact
    $signedBytes = if (Test-Path -LiteralPath $artifactPath) {
        (Get-Item -LiteralPath $artifactPath).Length
    } else {
        $null
    }

    [pscustomobject]@{
        Variant = $variant.Variant
        BringUpNCLOC = $bringUpLines
        UdpServerNCLOC = $serverLines
        StudyTotalNCLOC = $bringUpLines + $serverLines
        SignedBytes = $signedBytes
    }
}

$results | Format-Table -AutoSize
Write-Host ""
Write-Host "NCLOC excludes blank lines and full-line // comments; braces and declarations count."
Write-Host "Shared files count in every variant that must maintain and compile them."
Write-Host "Shared board.rs, tests, tools, bootloader, SSH, and update code are outside this scope."
