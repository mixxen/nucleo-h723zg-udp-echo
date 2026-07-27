[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

if (-not $SkipBuild) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty Source
    if (-not $cargo) {
        $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    }
    if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
        throw "Cargo was not found. Install Rust with rustup and restart VS Code."
    }

    Push-Location $projectRoot
    try {
        & $cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

$elf = Join-Path $projectRoot "target\thumbv7em-none-eabihf\release\nucleo-h723zg-udp-echo"
if (-not (Test-Path -LiteralPath $elf -PathType Leaf)) {
    throw "Firmware ELF not found at '$elf'. Run cargo build --release first."
}

$openOcdRoot = Join-Path $env:APPDATA "xPacks\@xpack-dev-tools\openocd"
$openOcd = Get-ChildItem -LiteralPath $openOcdRoot -Recurse -Filter openocd.exe -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $openOcd) {
    throw "OpenOCD was not found below '$openOcdRoot'. Install @xpack-dev-tools/openocd first."
}

$elfForOpenOcd = (Resolve-Path -LiteralPath $elf).Path.Replace("\", "/")

& $openOcd `
    -f interface/stlink.cfg `
    -f target/stm32h7x.cfg `
    -c "adapter speed 3300" `
    -c "program {$elfForOpenOcd} verify reset exit"

if ($LASTEXITCODE -ne 0) {
    throw "OpenOCD failed with exit code $LASTEXITCODE"
}

Write-Host "Firmware programmed, verified, reset, and started."
