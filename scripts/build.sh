#!/usr/bin/env sh
# Build a standalone bot_agent binary for Linux/macOS.
# Output: dist/bot_agent
#
# Usage: ./scripts/build.sh

set -e

# Always run from repo root
cd "$(dirname "$0")/.."

pip install pyinstaller --quiet

pyinstaller \
  --onefile \
  --name bot_agent \
  --collect-all zenoh \
  --collect-all anthropic \
  --collect-all httpx \
  --hidden-import zenoh \
  bot_agent.py

echo ""
echo "Build complete: dist/bot_agent"
echo "Run it with:"
echo "  BOT_NAME=mybot ANTHROPIC_API_KEY=sk-ant-... ./dist/bot_agent"
