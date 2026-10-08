@echo off
setlocal
rem One press: one question, then the uninstaller, which removes what Folio set
rem up outside this folder, and then this folder's Folio files once this window
rem has closed. The uninstaller's lines are in the language Folio is set to; this
rem script's own lines are printed in English, then in Chinese. The Chinese is
rem UTF-8, so the console reads this file as UTF-8 (code page 65001) from here
rem on, and the code page it had is put back before the script ends.
rem Its lines end in CRLF and it has no byte-order mark: cmd.exe reads a batch
rem file by CRLF lines, and would read a mark as part of the first command.
rem chcp is given nothing to read (<nul): it shares this script's input, and
rem reads what a file handed to the script holds, which would take the answer
rem before the question is asked.
for /f "tokens=*" %%a in ('chcp ^<nul') do for %%b in (%%a) do set "codepage=%%b"
set "codepage=%codepage:.=%"
chcp 65001 >nul <nul
echo Keep settings and data? [Y/n]
set "answer="
set /p "answer=保留设置和数据？[Y/n] "
if defined answer set "answer=%answer:"=%"
set "remove="
if /i "%answer%"=="n" set "remove=--remove-data"
if /i "%answer%"=="no" set "remove=--remove-data"
"%~dp0folio.exe" --uninstall %remove%
set "door=%errorlevel%"
if "%door%"=="2" (
    echo Folio is running. Quit Folio, then run uninstall.cmd again.
    echo Folio 正在运行。退出 Folio 后重新运行 uninstall.cmd。
)
if "%door%"=="0" (
    echo Removal continues after this window closes.
    echo 关闭此窗口后继续移除。
)
pause
chcp %codepage% >nul <nul
exit /b %door%
