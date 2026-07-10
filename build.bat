@echo off
echo ===================================================
echo   Building ContextSwitch Tauri Production Release
echo ===================================================
echo.

call npm run tauri build

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Build failed! Check the console output above.
    echo.
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo [SUCCESS] Build completed successfully!
echo Opening installer directory...
echo.

if exist "src-tauri\target\release\bundle\nsis" (
    explorer.exe "src-tauri\target\release\bundle\nsis"
) else if exist "src-tauri\target\release\bundle\msi" (
    explorer.exe "src-tauri\target\release\bundle\msi"
) else (
    explorer.exe "src-tauri\target\release"
)

pause
