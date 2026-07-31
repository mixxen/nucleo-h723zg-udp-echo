[CmdletBinding()]
param(
    [string]$ZephyrWorkspace = (Join-Path $env:USERPROFILE "zephyrproject-v4.4.0"),
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$bootloaderRoot = Split-Path -Parent $PSScriptRoot
$privateKey = Join-Path $bootloaderRoot "root-ed25519.pem"
$publicKey = Join-Path $bootloaderRoot "root-ed25519.pub.pem"
$imgtool = Join-Path $ZephyrWorkspace ".venv313\Scripts\imgtool.exe"

if (-not (Test-Path -LiteralPath $imgtool -PathType Leaf)) {
    throw "imgtool was not found at '$imgtool'. Set -ZephyrWorkspace to the pinned Zephyr workspace."
}

if ((Test-Path -LiteralPath $privateKey) -and -not $Force) {
    throw "A signing key already exists at '$privateKey'. Use -Force only when intentionally rotating the development key."
}

& $imgtool keygen -k $privateKey -t ed25519
if ($LASTEXITCODE -ne 0) {
    throw "imgtool failed to generate the signing key."
}

& $imgtool getpub -k $privateKey -e pem -o $publicKey
if ($LASTEXITCODE -ne 0) {
    throw "imgtool failed to export the public key."
}

Write-Host "Created the private development signing key:"
Write-Host "  $privateKey"
Write-Host "Keep it private and backed up. Rotating it requires rebuilding and reflashing MCUboot."
