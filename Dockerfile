# syntax=docker/dockerfile:1
# Tabnest 镜像：多阶段构建 —— 先用 Rust 官方镜像编译，再放进精简的 Debian 运行时。

# ---------- 构建阶段 ----------
FROM rust:1.92-slim-bookworm AS builder
WORKDIR /src

# 可选：Debian 软件源镜像（只替换主机名，保留 /debian 路径），
# 例如 ARG DEBIAN_MIRROR=mirrors.aliyun.com。
ARG DEBIAN_MIRROR=""
RUN if [ -n "$DEBIAN_MIRROR" ]; then \
        for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources; do \
            if [ -f "$f" ]; then \
                sed -i "s|deb.debian.org|$DEBIAN_MIRROR|g; s|security.debian.org|$DEBIAN_MIRROR|g" "$f"; \
            fi; \
        done; \
    fi \
    && apt-get -o Acquire::Retries=3 update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential \
    && rm -rf /var/lib/apt/lists/*

# 先复制清单并编译一次依赖：源码改动时不会触发整个依赖树重编译。
COPY Cargo.toml Cargo.lock ./

# 可选：crates.io 镜像，例如 CARGO_REGISTRY_MIRROR=https://rsproxy.cn/index/
ARG CARGO_REGISTRY_MIRROR=""
RUN if [ -n "$CARGO_REGISTRY_MIRROR" ]; then \
        mkdir -p .cargo && \
        printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "sparse+%s"\n' "$CARGO_REGISTRY_MIRROR" > .cargo/config.toml; \
    fi

# 用一个空的 main.rs 预编译全部依赖，并缓存 cargo 的 registry / git 目录。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# 复制真实源码重新构建；static/ 与 migrations/ 必须在场，因为它们是
# 通过 include_str! 在编译期嵌入二进制的。
COPY static ./static
COPY migrations ./migrations
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    touch src/main.rs && cargo build --release

# ---------- 运行时阶段 ----------
FROM debian:bookworm-slim

# 运行时同样支持 Debian 镜像；ca-certificates 用于抓取 favicon 的 HTTPS 请求，
# curl 供 HEALTHCHECK 使用，util-linux 提供入口脚本里的 setpriv。
ARG DEBIAN_MIRROR=""
RUN if [ -n "$DEBIAN_MIRROR" ]; then \
        for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources; do \
            if [ -f "$f" ]; then \
                sed -i "s|deb.debian.org|$DEBIAN_MIRROR|g; s|security.debian.org|$DEBIAN_MIRROR|g" "$f"; \
            fi; \
        done; \
    fi \
    && apt-get -o Acquire::Retries=3 update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates libssl3 curl util-linux tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin tabnest \
    && mkdir -p /data/favicons \
    && chown -R tabnest:tabnest /data

COPY --from=builder /src/target/release/tabnest /usr/local/bin/tabnest
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh

# 默认配置：数据目录、监听地址、日志级别（可用 -e 覆盖）。
ENV DATA_DIR=/data \
    BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=tabnest=info,sqlx=warn

VOLUME ["/data"]
EXPOSE 8080

# 健康检查直接打 /healthz，失败即标记容器不健康。
HEALTHCHECK --interval=30s --timeout=4s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
