@echo off
setlocal EnableDelayedExpansion
title NCAA Next Texture Installer (DEBUG)

echo ================================
echo NCAA Next Textures Downloader
echo DEBUG MODE
echo ================================
echo.

:: Script directory
set SCRIPT_DIR=%~dp0
echo Textures installation directory:
echo %SCRIPT_DIR%
echo.

:: Check if SLUS-21214 already exists
if exist "%SCRIPT_DIR%SLUS-21214" (
    echo.
    echo WARNING: A 'SLUS-21214' folder already exists.
    echo Delete or rename the folder and then run this script again.
    echo.
    pause
    exit /b
)

:: Architecture detection
@REM echo Detecting architecture...
@REM echo PROCESSOR_ARCHITECTURE=%PROCESSOR_ARCHITECTURE%
@REM echo PROCESSOR_ARCHITEW6432=%PROCESSOR_ARCHITEW6432%

set ARCH=
if /i "%PROCESSOR_ARCHITECTURE%"=="ARM64" set ARCH=arm64
if /i "%PROCESSOR_ARCHITECTURE%"=="AMD64" set ARCH=x64
if /i "%PROCESSOR_ARCHITEW6432%"=="AMD64" set ARCH=x64

if "%ARCH%"=="" (
    echo.
    echo ERROR: Could not determine CPU architecture.
    pause
    exit /b
)

@REM echo Using architecture: %ARCH%
@REM echo.

:: Try system git
echo Checking for system Git...
git --version
if errorlevel 1 (
    echo System Git NOT found.
    echo Will try bundled MinGit.
    echo.

    set "MINGIT_ROOT=%SCRIPT_DIR%mingit\%ARCH%"
    set "MINGIT_GIT=%SCRIPT_DIR%mingit\%ARCH%\cmd\git.exe"

    @REM echo Bundled MinGit root:
    @REM echo %SCRIPT_DIR%mingit\%ARCH%
    @REM echo Bundled git.exe path:
    @REM echo %SCRIPT_DIR%mingit\%ARCH%\cmd\git.exe
    @REM echo.

    if not exist "%SCRIPT_DIR%mingit\%ARCH%\cmd\git.exe" (
        echo.
        echo ERROR: Bundled git.exe not found!
        echo Expected:
        echo %SCRIPT_DIR%mingit\%ARCH%\cmd\git.exe
        pause
        exit /b
    )

    @REM echo Adding MinGit to PATH...
    set "PATH=%SCRIPT_DIR%mingit\%ARCH%\cmd;%SCRIPT_DIR%mingit\%ARCH%\usr\bin;%PATH%"
    @REM echo PATH is now:
    @REM echo %PATH%
    @REM echo.

    set USING_BUNDLED=1
) else (
    echo System Git found and will be used.
    echo.
    set USING_BUNDLED=0
)

:: Test git explicitly
echo Testing git command:
git --version
if errorlevel 1 (
    echo.
    echo ERROR: git still cannot run!
    pause
    exit /b
)

echo.
echo Git works. Continuing...
echo.

:: Temp folder
set TEMP_REPO=_temp_ncaa_repo

echo Temp repo folder: %TEMP_REPO%

if exist "%TEMP_REPO%" (
    echo Removing existing temp folder...
    rmdir /s /q "%TEMP_REPO%"
)

echo.
echo Setting up local repository...
echo.
git clone --depth=1 --filter=blob:none --sparse ^
https://github.com/ncaanext/ncaa-next-26.git "%TEMP_REPO%"

if errorlevel 1 (
    echo.
    echo ERROR: git clone failed!
    pause
    exit /b
)

echo.
echo Repo setup. Downloading textures...
echo.

cd "%TEMP_REPO%"
if errorlevel 1 (
    echo ERROR: Could not cd into %TEMP_REPO%
    pause
    exit /b
)

echo.
echo Downloading textures...
echo.

git sparse-checkout set textures/SLUS-21214

if errorlevel 1 (
    echo.
    echo ERROR: sparse-checkout failed!
    pause
    exit /b
)

echo.
echo Download successful.
echo.

cd ..
if errorlevel 1 (
    echo ERROR: Could not return to parent directory.
    pause
    exit /b
)

echo.
echo Moving SLUS-21214 folder...
move "%TEMP_REPO%\textures\SLUS-21214" "SLUS-21214"

if errorlevel 1 (
    echo.
    echo ERROR: Move failed.
    pause
    exit /b
)

echo.
echo Deleting temp repo...
rmdir /s /q "%TEMP_REPO%"

:: Remove stray "Texture" file if it exists
if exist "%SCRIPT_DIR%Texture" (
    @REM echo Removing stray Texture file...
    del "%SCRIPT_DIR%Texture"
)

echo.
echo ================================
echo SUCCESS!
echo Your texures are installed in the SLUS-21214 folder located here:
echo %CD%
echo.
echo Ensure that path matches the path in PCSX2 settings > Graphics > Texture Replacement.
echo.
echo IMPORTANT: For future updates, use the Textures Downloader tool:
echo http://textures.ncaanext.com 
echo.
echo All done. Press any key to exit. You can delete this script and the 'mingit' folder.
echo ================================

pause

exit /b


