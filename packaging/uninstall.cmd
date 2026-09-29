@echo off
setlocal
rem Each line is printed in English, then in Chinese. The Chinese is UTF-8, so the
rem console reads this file as UTF-8 (code page 65001) from here on, and the code
rem page it had is put back before the script ends.
for /f "tokens=*" %%a in ('chcp') do for %%b in (%%a) do set "codepage=%%b"
set "codepage=%codepage:.=%"
chcp 65001 >nul
"%~dp0folio.exe" --uninstall-cleanup
set "cleanup_exit=%errorlevel%"
echo Cleanup exit code: %cleanup_exit%
echo 清理退出码：%cleanup_exit%
if "%cleanup_exit%"=="0" (
    echo You can now delete the Folio application folder. Your settings and data were kept.
    echo 现在可以删除 Folio 应用文件夹。设置和数据已保留。
) else (
    echo Cleanup did not complete. Read the result above before deleting the folder.
    echo 清理未完成。删除文件夹前请查看上方结果。
)
pause
chcp %codepage% >nul
exit /b %cleanup_exit%
