#!/usr/bin/env bash
# Build the signaling Worker (wrangler runs this before dev and deploy).
# SKIP_SIGNAL_BUILD=1 reuses an existing build (CI jobs that downloaded one).
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${SKIP_SIGNAL_BUILD:-}" = 1 ] && [ -f crates/signal/build/worker/shim.mjs ]; then
  echo "using existing signaling Worker build"
  exit 0
fi
command -v worker-build >/dev/null || cargo install worker-build --version 0.8.7 --locked
cd crates/signal && worker-build --release
