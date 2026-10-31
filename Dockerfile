# syntax=docker/dockerfile:1

# ---------- build ----------
FROM rust:1.92-slim-bookworm AS builder
WORKDIR /src

# Optional Debian mirror, e.g. mirrors.aliyun.com (keeps the /debian paths).
ARG DEBIAN_MIRROR=""
RUN if [ -n "$DEBIAN_MIRROR" ]; then \
        for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources; do \
            if [ -f "$f" ]; then \
                sed -i "s|deb.debian.org|$DEBIAN_MIRROR|g; s|security.debian.org|$DEBIAN_MIRROR|g" "$f"; \
            fi; \
        done; \
    fi \
    && apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential \
    && rm -rf /var/lib/apt/lists/*

# Compile dependencies first so source edits do not rebuild the world.
COPY Cargo.toml Cargo.lock ./

# Optional crates.io mirror, e.g. https://rsproxy.cn/index/
ARG CARGO_REGISTRY_MIRROR=""
RUN if [ -n "$CARGO_REGISTRY_MIRROR" ]; then \
        mkdir -p .cargo && \
        printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "sparse+%s"\n' "$CARGO_REGISTRY_MIRROR" > .cargo/config.toml; \
    fi

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY static ./static
COPY migrations ./migrations
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    touch src/main.rs && cargo build --release

# ---------- runtime ----------
FROM debian:bookworm-slim

ARG DEBIAN_MIRROR=""
RUN if [ -n "$DEBIAN_MIRROR" ]; then \
        for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources; do \
            if [ -f "$f" ]; then \
                sed -i "s|deb.debian.org|$DEBIAN_MIRROR|g; s|security.debian.org|$DEBIAN_MIRROR|g" "$f"; \
            fi; \
        done; \
    fi \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 curl util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin tabnest \
    && mkdir -p /data/favicons \
    && chown -R tabnest:tabnest /data

COPY --from=builder /src/target/release/tabnest /usr/local/bin/tabnest
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh

ENV DATA_DIR=/data \
    BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=tabnest=info,sqlx=warn

VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=4s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["tabnest"]
