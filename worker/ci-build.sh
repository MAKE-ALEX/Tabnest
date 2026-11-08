#!/usr/bin/env bash
# Build step for Cloudflare Workers Builds.
#
# Worker root directory: /worker
# Build command:          bash ci-build.sh
# Deploy command:         npx wrangler deploy
#
# Cloudflare's build image has Node but no Rust, so this installs rustup,
# the wasm target and `worker-build` (the latter compiles once; enable "构建缓存"
# so later builds reuse it instead of spending ~15 minutes again).
set -euo pipefail

WORKER_BUILD_VERSION=0.8.7

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found, installing a minimal Rust toolchain"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

if command -v rustup >/dev/null 2>&1; then
  rustup target add wasm32-unknown-unknown
fi

npm ci --no-audit --no-fund

if ! command -v worker-build >/dev/null 2>&1; then
  cargo install worker-build --locked --version "$WORKER_BUILD_VERSION"
fi

# Build the wasm bundle during the build step: the deploy command runs in a
# fresh shell without ~/.cargo/bin on PATH.
worker-build --release
