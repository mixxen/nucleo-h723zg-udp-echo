[CmdletBinding()]
param(
    # An existing OpenSSH Ed25519 public-key file may be supplied instead of
    # generating a project-local client key.
    [string]$AuthorizedPublicKey
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$keyDirectory = Join-Path $projectRoot ".ssh"
$hostSeedPath = Join-Path $keyDirectory "host_ed25519.seed"
$clientKeyPath = Join-Path $keyDirectory "client_ed25519"
$authorizedHexPath = Join-Path $keyDirectory "authorized_ed25519.hex"

[void](New-Item -ItemType Directory -Path $keyDirectory -Force)

# The 32 random bytes become the device's persistent Ed25519 signing-key seed.
# Do not regenerate this file for an already deployed board: SSH clients use
# the resulting public key to recognize the server across connections.
if (-not (Test-Path -LiteralPath $hostSeedPath -PathType Leaf)) {
    $seed = [byte[]]::new(32)
    $random = [Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $random.GetBytes($seed)
    }
    finally {
        $random.Dispose()
    }
    $seedHex = -join ($seed | ForEach-Object { $_.ToString("x2") })
    Set-Content -LiteralPath $hostSeedPath -Value $seedHex -NoNewline
    Write-Host "Generated a persistent device host key seed."
}

if ($AuthorizedPublicKey) {
    $publicKeyPath = (Resolve-Path -LiteralPath $AuthorizedPublicKey).Path
}
else {
    $publicKeyPath = "$clientKeyPath.pub"
    if (-not (Test-Path -LiteralPath $publicKeyPath -PathType Leaf)) {
        $sshKeygen = Get-Command ssh-keygen -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty Source
        if (-not $sshKeygen) {
            throw "ssh-keygen was not found. Install the Windows OpenSSH Client feature."
        }

        # Windows PowerShell 5 drops an ordinary empty native-command
        # argument, while PowerShell 7 preserves it. Quote according to the
        # host so `ssh-keygen` always receives an empty passphrase.
        if ($PSVersionTable.PSEdition -eq "Core") {
            & $sshKeygen -q -t ed25519 -N "" -C "nucleo-h723zg" -f $clientKeyPath
        }
        else {
            & $sshKeygen -q -t ed25519 -N '""' -C "nucleo-h723zg" -f $clientKeyPath
        }
        if ($LASTEXITCODE -ne 0) {
            throw "ssh-keygen failed with exit code $LASTEXITCODE"
        }
        Write-Host "Generated a project-local Ed25519 client key."
    }
}

# An OpenSSH Ed25519 public key is a base64 SSH wire blob ending in the raw
# 32-byte public key. Sunset compares that fixed-size value during login.
$fields = (Get-Content -LiteralPath $publicKeyPath -Raw).Trim() -split "\s+"
if ($fields.Length -lt 2 -or $fields[0] -ne "ssh-ed25519") {
    throw "'$publicKeyPath' is not an OpenSSH Ed25519 public key."
}
$wireKey = [Convert]::FromBase64String($fields[1])
if ($wireKey.Length -lt 32) {
    throw "'$publicKeyPath' contains a truncated public key."
}
$rawKey = $wireKey[($wireKey.Length - 32)..($wireKey.Length - 1)]
$publicHex = -join ($rawKey | ForEach-Object { $_.ToString("x2") })
Set-Content -LiteralPath $authorizedHexPath -Value $publicHex -NoNewline

Write-Host "Authorized public key: $publicKeyPath"
if (-not $AuthorizedPublicKey) {
    Write-Host "Connect with: ssh -i `"$clientKeyPath`" -p 2222 board@BOARD_IP"
}
