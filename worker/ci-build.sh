#!/usr/bin/env bash
# Build step for Cloudflare Workers Builds.
#
# Worker root directory: /worker
# Build command:          bash ci-build.sh
# Deploy command:         npx wrangler deploy
#
# The wasm target and `worker-build` are installed once; with build caching
# enabled Cloudflare reuses them, so later builds take a couple of minutes
# instead of ~15.
set -euo pipefail

if command -v rustup >/dev/null 2>&1; then
  rustup target add wasm32-unknown-unknown
fi

npm ci --no-audit --no-fund

if ! command -v worker-build >/dev/null 2>&1; then
  cargo install worker-build --locked --version 0.8.7
fi

worker-build --version
