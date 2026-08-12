[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ipaddress]$BoardIp,

    [ValidateRange(1, 100000)]
    [int]$Count = 25,

    [ValidateRange(100, 30000)]
    [int]$TimeoutMilliseconds = 3000,

    [string]$Sizes = "0,1,32,256,1472"
)

$ErrorActionPreference = "Stop"
$udp = [Net.Sockets.UdpClient]::new()
$udp.Client.ReceiveTimeout = $TimeoutMilliseconds
$remote = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
$testSizes = @($Sizes.Split(",", [StringSplitOptions]::RemoveEmptyEntries) | ForEach-Object {
    $parsed = 0
    if (-not [int]::TryParse($_.Trim(), [ref]$parsed)) {
        throw "Invalid payload size '$_'. Use a comma-separated list such as 1,32,256,1472."
    }
    $parsed
})
if ($testSizes.Count -eq 0) {
    throw "-Sizes must contain at least one payload size."
}
foreach ($size in $testSizes) {
    if ($size -lt 0 -or $size -gt 1472) {
        throw "Payload size $size is outside the supported range 0..1472."
    }
}
$random = [Random]::new(0x723)

try {
    for ($sequence = 0; $sequence -lt $Count; $sequence++) {
        $size = $testSizes[$sequence % $testSizes.Count]
        $payload = [byte[]]::new($size)
        $random.NextBytes($payload)

        if ($size -ge 4) {
            [BitConverter]::GetBytes($sequence).CopyTo($payload, 0)
        }

        [void]$udp.Send($payload, $payload.Length, $BoardIp, 7)
        $reply = $udp.Receive([ref]$remote)

        if (-not $remote.Address.Equals($BoardIp) -or $remote.Port -ne 7) {
            throw "Packet $sequence came from unexpected endpoint $remote"
        }

        if ($reply.Length -ne $payload.Length) {
            throw "Packet $sequence length mismatch: sent $($payload.Length), received $($reply.Length)"
        }

        for ($index = 0; $index -lt $payload.Length; $index++) {
            if ($reply[$index] -ne $payload[$index]) {
                throw "Packet $sequence payload mismatch at byte $index"
            }
        }
    }
}
finally {
    $udp.Dispose()
}

Write-Host "PASS: $Count UDP datagrams echoed byte-for-byte by ${BoardIp}:7."
Write-Host "Tested payload sizes: $($testSizes -join ', ') bytes."
