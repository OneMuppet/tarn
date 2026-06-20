#!/usr/bin/env bash
# Render the whitepaper HTML to PDF via headless Chrome.
set -euo pipefail
cd "$(dirname "$0")"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
"$CHROME" --headless --disable-gpu --no-pdf-header-footer \
  --print-to-pdf="tarn-whitepaper.pdf" "file://$PWD/tarn-whitepaper.html"
echo "wrote tarn-whitepaper.pdf"
