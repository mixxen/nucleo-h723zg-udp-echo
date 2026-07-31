[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$HostName,

    [Parameter(Mandatory)]
    [string]$KeyPath,

    [string]$ImagePath,

    [ValidateRange(1, 65535)]
    [int]$Port = 2222,

    [string]$UserName = "board"
)

$ErrorActionPreference = "Stop"
$maximumImageSize = 0x40000

# PowerShell can evaluate parameter defaults before `$PSScriptRoot` is
# populated. Resolve the repository-relative default in the script body,
# where the directory is guaranteed to be available.
if (-not $ImagePath) {
    $ImagePath = Join-Path $PSScriptRoot "..\artifacts\firmware-signed.bin"
}

$image = (Resolve-Path -LiteralPath $ImagePath).Path
$key = (Resolve-Path -LiteralPath $KeyPath).Path
$length = (Get-Item -LiteralPath $image).Length

if ($length -lt 0x200 -or $length -gt $maximumImageSize) {
    throw "Signed image size $length is outside the allowed 512..$maximumImageSize byte range."
}

$sha256 = [System.Security.Cryptography.SHA256]::Create()
try {
    $inputFile = [System.IO.File]::OpenRead($image)
    try {
        $digest = $sha256.ComputeHash($inputFile)
    }
    finally {
        $inputFile.Dispose()
    }
}
finally {
    $sha256.Dispose()
}

# Header: "FWUP", protocol version 1, three reserved zero bytes, a
# little-endian u32 length, and the SHA-256 digest.
$header = New-Object byte[] 44
[System.Text.Encoding]::ASCII.GetBytes("FWUP").CopyTo($header, 0)
$header[4] = 1
[System.BitConverter]::GetBytes([uint32]$length).CopyTo($header, 8)
$digest.CopyTo($header, 12)

$ssh = Get-Command ssh.exe -ErrorAction Stop
$processInfo = New-Object System.Diagnostics.ProcessStartInfo
$processInfo.FileName = $ssh.Source
$processInfo.Arguments = (
    "-T -o BatchMode=yes -o StrictHostKeyChecking=accept-new " +
    "-p $Port -i `"$key`" -s $UserName@$HostName firmware-update"
)
$processInfo.UseShellExecute = $false
$processInfo.RedirectStandardInput = $true
$processInfo.RedirectStandardOutput = $true
$processInfo.RedirectStandardError = $true
$processInfo.CreateNoWindow = $true

$process = New-Object System.Diagnostics.Process
$process.StartInfo = $processInfo
if (-not $process.Start()) {
    throw "Failed to start ssh.exe."
}

try {
    $process.StandardInput.BaseStream.Write($header, 0, $header.Length)
    $process.StandardInput.BaseStream.Flush()

    do {
        $response = $process.StandardOutput.ReadLine()
        if ($null -eq $response) {
            throw "SSH closed before the board became ready: $($process.StandardError.ReadToEnd())"
        }
        Write-Host $response
        if ($response.StartsWith("ERR ")) {
            throw "Board rejected the upload: $response"
        }
    } until ($response -eq "READY")

    $sent = 0L
    $inputFile = [System.IO.File]::OpenRead($image)
    try {
        $buffer = New-Object byte[] 1024
        while (($count = $inputFile.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $process.StandardInput.BaseStream.Write($buffer, 0, $count)
            $sent += $count
        }
        $process.StandardInput.BaseStream.Flush()
        $process.StandardInput.Close()
    }
    finally {
        $inputFile.Dispose()
    }

    $accepted = $false
    while (($response = $process.StandardOutput.ReadLine()) -ne $null) {
        Write-Host $response
        if ($response.StartsWith("ERR ")) {
            throw "Board rejected the upload: $response"
        }
        if ($response.StartsWith("OK ")) {
            $accepted = $true
            break
        }
    }

    if (-not $accepted) {
        $process.WaitForExit()
        $details = $process.StandardError.ReadToEnd()
        throw "SSH ended without the board's success record (exit $($process.ExitCode)): $details"
    }

    # `OK` is the protocol's durable success boundary: the image and activation
    # markers have been verified in flash. The board resets immediately after
    # sending it, and Windows OpenSSH does not always notice that half-closed
    # TCP session promptly. Give it a moment to exit cleanly, then reap it
    # ourselves instead of turning a successful board update into a hang.
    if (-not $process.WaitForExit(5000)) {
        $process.Kill()
        $process.WaitForExit()
    }

    if ($sent -ne $length) {
        throw "Only $sent of $length bytes were sent."
    }
}
finally {
    if (-not $process.HasExited) {
        $process.Kill()
    }
    $process.Dispose()
}

Write-Host "Firmware accepted. The board is rebooting into an MCUboot trial image."
