@echo off
setlocal
rem CHINESE PENDING: archive cleanup instructions.
"%~dp0folio.exe" --uninstall-cleanup
set "cleanup_exit=%errorlevel%"
echo Cleanup exit code: %cleanup_exit%
if "%cleanup_exit%"=="0" (
    echo You can now delete the Folio application folder. Your settings and data were kept.
) else (
    echo Cleanup did not complete. Read the result above before deleting the folder.
)
pause
exit /b %cleanup_exit%
