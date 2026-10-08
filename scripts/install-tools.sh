#!/usr/bin/env bash
# Install the web build tools into .tools/bin (cloud session or CI only).
# wasm-bindgen-cli must match the wasm-bindgen version in Cargo.lock exactly.
set -euo pipefail
cd "$(dirname "$0")/.."

BINARYEN_VERSION=125
TOOLS=.tools/bin
mkdir -p "$TOOLS"

WB_VERSION=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/,""); print; exit}' Cargo.lock)
if [ -z "$WB_VERSION" ]; then
  echo "wasm-bindgen not found in Cargo.lock" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if ! "$TOOLS/wasm-bindgen" --version 2>/dev/null | grep -q " $WB_VERSION\$"; then
  echo "installing wasm-bindgen $WB_VERSION"
  name="wasm-bindgen-$WB_VERSION-x86_64-unknown-linux-musl"
  curl -sSLf "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$WB_VERSION/$name.tar.gz" | tar xz -C "$tmp"
  cp "$tmp/$name/wasm-bindgen" "$TOOLS/"
fi

if ! "$TOOLS/wasm-opt" --version 2>/dev/null | grep -q "version $BINARYEN_VERSION"; then
  echo "installing binaryen $BINARYEN_VERSION (wasm-opt)"
  curl -sSLf "https://github.com/WebAssembly/binaryen/releases/download/version_$BINARYEN_VERSION/binaryen-version_$BINARYEN_VERSION-x86_64-linux.tar.gz" | tar xz -C "$tmp"
  cp "$tmp/binaryen-version_$BINARYEN_VERSION/bin/wasm-opt" "$TOOLS/"
  mkdir -p .tools/lib
  cp -r "$tmp/binaryen-version_$BINARYEN_VERSION/lib/." .tools/lib/ 2>/dev/null || true
fi

rustup target add wasm32-unknown-unknown >/dev/null
"$TOOLS/wasm-bindgen" --version
"$TOOLS/wasm-opt" --version
