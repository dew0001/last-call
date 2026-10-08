#!/usr/bin/env bash
# Run the Playwright suite the way CI does: on a virtual display (Firefox
# needs a window for WebGL) with a silent PulseAudio sink (Firefox cannot
# start Web Audio without an output device). Extra args go to Playwright.
set -euo pipefail
cd "$(dirname "$0")/.."

if command -v pulseaudio >/dev/null; then
  if ! pactl info >/dev/null 2>&1; then
    pulseaudio --start --exit-idle-time=-1 >/dev/null 2>&1 || true
  fi
  pactl load-module module-null-sink sink_name=null >/dev/null 2>&1 || true
  server=$(pactl info 2>/dev/null | sed -n 's/Server String: //p')
  if [ -n "$server" ]; then
    export PULSE_SERVER="unix:$server"
  fi
else
  echo "note: pulseaudio not installed; the Firefox voice test will fail" >&2
fi

exec xvfb-run -a npx playwright test "$@"
