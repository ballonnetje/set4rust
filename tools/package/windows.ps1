# Builds the Windows packages into target\package:
#   S4RS-Setup-<version>.exe                  installer (Inno Setup 6)
#   S4RS-Windows-x64-<version>.zip            portable game: launcher + game
#   S4RS-Asset-Studio-Windows-x64-<version>.zip   the Asset Studio
# The exes use a static C runtime, so no VC++ redistributable is needed.
# Usage (from anywhere): powershell -ExecutionPolicy Bypass -File tools\package\windows.ps1
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$pkg = Join-Path $root 'target\package'
$buildDir = Join-Path $root 'target\package-build'

$toml = Get-Content (Join-Path $root 'crates\s4-game\Cargo.toml') -Raw
if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'no version in crates\s4-game\Cargo.toml' }
$version = $Matches[1]
Write-Host "S4RS $version"

$env:RUSTFLAGS = '-C target-feature=+crt-static'
$env:CARGO_TARGET_DIR = $buildDir
try {
    Push-Location $root
    cargo build --release --locked -p s4-game -p s4-portable --features s4-game/package
    if ($LASTEXITCODE -ne 0) { throw "cargo build of the game failed ($LASTEXITCODE)" }
    Pop-Location
    Push-Location (Join-Path $root 'studio')
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build of the Asset Studio failed ($LASTEXITCODE)" }
    Pop-Location
} finally {
    Remove-Item Env:RUSTFLAGS, Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
}
$game = Join-Path $buildDir 'release\s4-game.exe'
$launcher = Join-Path $buildDir 'release\s4-portable.exe'
$studio = Join-Path $buildDir 'release\s4-asset-studio.exe'

New-Item -ItemType Directory -Force $pkg | Out-Null

function New-Zip([string]$top, [hashtable]$files, [string]$zipName) {
    $stage = Join-Path $pkg 'stage'
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    $dir = Join-Path $stage $top
    New-Item -ItemType Directory -Force $dir | Out-Null
    foreach ($name in $files.Keys) { Copy-Item $files[$name] (Join-Path $dir $name) }
    $zip = Join-Path $pkg $zipName
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $dir -DestinationPath $zip -CompressionLevel Optimal
    Remove-Item -Recurse -Force $stage
    Write-Host ('{0}: {1:N1} MB' -f $zip, ((Get-Item $zip).Length / 1MB))
}

New-Zip 'S4RS' @{
    'S4RS.exe'        = $launcher
    's4-game.exe'     = $game
    'S4RS-README.txt' = (Join-Path $PSScriptRoot 'README-windows-portable.txt')
} "S4RS-Windows-x64-$version.zip"

New-Zip 'S4RS-Asset-Studio' @{
    's4-asset-studio.exe' = $studio
    'README.txt'          = (Join-Path $PSScriptRoot 'README-asset-studio.txt')
} "S4RS-Asset-Studio-Windows-x64-$version.zip"

$iscc = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install -e --id JRSoftware.InnoSetup --scope user)' }
& $iscc "/DAppVersion=$version" "/DExe=$game" "/DOutDir=$pkg" (Join-Path $PSScriptRoot 's4rs.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }
$out = Join-Path $pkg "S4RS-Setup-$version.exe"
Write-Host ('{0}: {1:N1} MB' -f $out, ((Get-Item $out).Length / 1MB))
