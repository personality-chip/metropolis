@echo off
setlocal
cd /d "%~dp0"
metropolis.exe --apps %*
if errorlevel 1 pause
