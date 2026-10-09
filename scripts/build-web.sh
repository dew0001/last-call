#!/usr/bin/env bash
# Build the web bundle into dist/.
#
#   scripts/build-web.sh [debug|release]
#
# Produces three wasm modules:
#   pkg/client_webgl2  the client with the WebGL2 renderer
#   pkg/client_webgpu  the client with the WebGPU renderer
#   pkg/host           the host simulation, booted by host-worker.js in a Web Worker
# Bevy's `webgpu` feature replaces WebGL2 instead of adding to it, so the client
# ships twice and web/index.html picks one at load time.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE=${1:-debug}
case "$MODE" in
  debug) PROFILE=dev; OUT=debug ;;
  release) PROFILE=release; OUT=release ;;
  *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac

./scripts/install-tools.sh >/dev/null
TOOLS=.tools/bin
TARGET=wasm32-unknown-unknown
DIST=dist
rm -rf "$DIST"
mkdir -p "$DIST/pkg"

bindgen() { # <wasm file> <out name>
  "$TOOLS/wasm-bindgen" --target web --no-typescript --out-dir "$DIST/pkg" --out-name "$2" "$1"
  if [ "$MODE" = release ]; then
    "$TOOLS/wasm-opt" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
      --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue --enable-simd \
      -o "$DIST/pkg/$2_bg.wasm" "$DIST/pkg/$2_bg.wasm"
  fi
}

echo "== client (webgl2)"
cargo build --profile "$PROFILE" --target "$TARGET" -p last_call_client --lib
bindgen "target/$TARGET/$OUT/client.wasm" client_webgl2

if [ "${SKIP_WEBGPU:-}" = 1 ]; then
  # Fast local iteration only; pages must then use ?gpu=webgl2.
  echo "== client (webgpu) skipped (SKIP_WEBGPU=1)"
else
  echo "== client (webgpu)"
  # Separate target dir: switching the feature would otherwise rebuild Bevy every time.
  cargo build --profile "$PROFILE" --target "$TARGET" -p last_call_client --lib --features webgpu --target-dir target/webgpu
  bindgen "target/webgpu/$TARGET/$OUT/client.wasm" client_webgpu
fi

echo "== host"
# WebAssembly SIMD: glam's SIMD paths then round like its SSE2 paths on a
# native x86-64 build, so the browser host and the native host simulate bit for
# bit alike (crates/host/tests/replay.rs). Own target dir: other flags would
# otherwise rebuild everything for the client too.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
  cargo build --profile "$PROFILE" --target "$TARGET" -p last_call_host --lib --target-dir target/host-simd
bindgen "target/host-simd/$TARGET/$OUT/host.wasm" host

cp -r web/. "$DIST/"
echo "$MODE" > "$DIST/build-mode.txt"

echo "== sizes"
if [ "$MODE" = release ]; then
  node scripts/size-report.mjs "$DIST" --enforce
else
  node scripts/size-report.mjs "$DIST"
fi
