#!/usr/bin/env bash
# Cloudflare Workers Builds 的构建脚本。
#
# Worker 根目录：/worker
# 构建命令：      bash ci-build.sh
# 部署命令：      npx wrangler deploy
#
# Cloudflare 的构建镜像只有 Node、没有 Rust，因此这里需要自行安装 rustup、
# wasm 目标和 `worker-build`。worker-build 只编译一次；建议在控制台开启
# 「构建缓存」，否则每次构建都要再花约 15 分钟。
set -euo pipefail

# 固定 worker-build 版本，避免上游更新导致构建产物漂移。
WORKER_BUILD_VERSION=0.8.7

# 第一步：没有 cargo 就装一套最小的 stable 工具链。
if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found, installing a minimal Rust toolchain"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# 第二步：Workers 产物需要 wasm32 目标。
if command -v rustup >/dev/null 2>&1; then
  rustup target add wasm32-unknown-unknown
fi

# 第三步：按 lockfile 安装 wrangler 等 npm 依赖。
npm ci --no-audit --no-fund

# 第四步：安装 worker-build（若未预装）。
if ! command -v worker-build >/dev/null 2>&1; then
  cargo install worker-build --locked --version "$WORKER_BUILD_VERSION"
fi

# 在构建阶段就产出 wasm 包：部署命令运行在一个全新的 shell 里，
# PATH 中没有 ~/.cargo/bin，届时再编译会找不到工具。
worker-build --release
