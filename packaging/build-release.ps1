param(
    [string]$Out = "dist\soe-play-windows"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

cargo build -p soe-app --release

if (Test-Path $Out) {
    Remove-Item -Recurse -Force $Out
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Copy-Item "target\release\soe-play.exe" $Out
Copy-Item "README.md" $Out
Copy-Item "notes\PLAY.md" (Join-Path $Out "PLAY.md")

@'
Spirit of Excalibur - Rust reimplementation
==========================================

Requirements
------------
A legally owned copy of the original DOS game in a `game` folder next to
`soe-play.exe`, or set SOE_GAME_DIR to that folder.

Expected layout:

  game\EPISODE1.DAT
  game\SCENES.RES
  game\HINT.RES
  game\RES32\*.RES
  game\*.SAV              (optional saves)

EXCAL_UNPACKED.EXE is only needed for palettes/font metrics. Set SOE_EXE to
its path, or place it at ghidra\EXCAL_UNPACKED.EXE relative to the executable.

Controls
--------
arrows/click  scene or menu selection
M             map
I             status
A             action menu
T             travel
S             saves
H             story chronicle
P             pause
F5 / F9       quick save / load
Space / R     animation step / reset
Esc           quit

This package contains no original game files, no unpacked executables and no
converted assets.
'@ | Set-Content -Encoding ASCII (Join-Path $Out "QUICKSTART.txt")

$forbidden = Get-ChildItem -Recurse -File $Out | Where-Object {
    $_.Extension -match '^\.(exe|res|dvr|sav|dat|png|bmp|gif|zip)$' -and
    $_.Name -ne 'soe-play.exe'
}
if ($forbidden) {
    throw "Release directory contains forbidden game/reference files: $($forbidden.FullName -join ', ')"
}

Write-Host "Packaged to $Out"
