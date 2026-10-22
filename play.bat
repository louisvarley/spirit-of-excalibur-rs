@echo off
rem Build the latest code and play.
rem Needs the original game files in game\ and ghidra\EXCAL_UNPACKED.EXE
rem (or set SOE_GAME_DIR / SOE_EXE). Extra arguments are passed through,
rem e.g. play.bat --no-intro
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

if not exist "game\SCENES.RES" if not defined SOE_GAME_DIR (
    echo Original game files not found in "%~dp0game".
    echo Copy your game there or set SOE_GAME_DIR.
    pause
    exit /b 1
)

echo Building soe-play (release)...
cargo build --release -p soe-app
if errorlevel 1 (
    echo.
    echo Build failed.
    pause
    exit /b 1
)

start "" "%~dp0target\release\soe-play.exe" %*
endlocal
