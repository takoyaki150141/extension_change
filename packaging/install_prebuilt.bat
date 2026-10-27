@echo off
setlocal
cd /d "%~dp0"

echo.
echo Extension Change
echo Installing...
echo.

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install_prebuilt.ps1"
set "EXIT_CODE=%ERRORLEVEL%"

echo.
if "%EXIT_CODE%"=="0" (
  echo Installation finished.
) else (
  echo Installation failed. Exit code: %EXIT_CODE%
)
echo.
pause
exit /b %EXIT_CODE%
