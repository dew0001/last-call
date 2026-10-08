# LAST CALL. Runs in the cloud session and in CI. Nothing here runs on a player's PC.
#   make dev    build the debug web bundle, then serve it on :8080 and run the signaling Worker on :8787
#   make test   everything in section 11 of the plan that exists so far
#   make build  release web bundle in dist/ and the signaling Worker bundle

SHELL := /bin/bash
export PATH := $(CURDIR)/.tools/bin:$(HOME)/.cargo/bin:$(PATH)

.PHONY: dev test build tools node-deps signal web-debug web-release lint unit e2e clean

tools:
	./scripts/install-tools.sh

node-deps:
	@[ -d node_modules ] || npm ci

signal:
	@command -v worker-build >/dev/null || cargo install worker-build --version 0.8.7 --locked
	cd crates/signal && worker-build --release

web-debug:
	./scripts/build-web.sh debug

web-release:
	./scripts/build-web.sh release

dev: node-deps web-debug signal
	@trap 'kill 0' EXIT; \
	node scripts/serve.mjs dist 8080 & \
	npx wrangler dev --port 8787 --ip 127.0.0.1 & \
	wait

lint:
	cargo fmt --all --check
	cd crates/signal && cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --target wasm32-unknown-unknown -p last_call_client -p last_call_host --lib -- -D warnings
	cd crates/signal && cargo clippy --target wasm32-unknown-unknown -- -D warnings

unit:
	cargo test --workspace

# PW_BROWSERS limits browsers (the cloud session can only run chromium).
e2e: node-deps
	npx playwright test

test: lint unit web-debug e2e

build: web-release signal

clean:
	rm -rf dist crates/signal/build test-results playwright-report
