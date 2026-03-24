@echo off
REM Build a standalone bot_agent.exe for Windows.
REM Output: dist\bot_agent.exe
REM
REM Usage: scripts\build.bat

cd /d "%~dp0\.."

pip install pyinstaller --quiet

pyinstaller ^
  --onefile ^
  --name bot_agent ^
  --collect-all zenoh ^
  --collect-all anthropic ^
  --collect-all httpx ^
  --hidden-import zenoh ^
  bot_agent.py

echo.
echo Build complete: dist\bot_agent.exe
echo Run it with:
echo   set BOT_NAME=mybot ^&^& set ANTHROPIC_API_KEY=sk-ant-... ^&^& dist\bot_agent.exe
