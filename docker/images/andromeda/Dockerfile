# syntax=docker/dockerfile:1

FROM rust:1.99.1-trixie AS chef

# cmake for aws-lc-sys.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef --locked --version 0.1.78

WORKDIR /build

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

# Must precede any cargo call.
COPY rust-toolchain.toml ./
COPY .cargo ./.cargo
RUN rustup show

# Survives the lockfile changes that invalidate the chef layer.
ARG SCCACHE_VERSION=0.18.0
RUN set -eux; \
    arch="$(uname -m)"; \
    case "$arch" in \
        x86_64) sha256=45f1447fbe231e3037bde351ef70677dd212216c8d62ae7ca409fecc4d6acc89 ;; \
        aarch64) sha256=2b3284d5da3b46a47dc4229e75bb7b88ac4aa99c8d754fb7d2f84997e5a4354a ;; \
        *) echo "no pinned sccache checksum for $arch" >&2; exit 1 ;; \
    esac; \
    release="sccache-v${SCCACHE_VERSION}-${arch}-unknown-linux-musl"; \
    curl -fsSL -o /tmp/sccache.tar.gz \
        "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/${release}.tar.gz"; \
    echo "${sha256}  /tmp/sccache.tar.gz" | sha256sum -c -; \
    tar -xzf /tmp/sccache.tar.gz -C /tmp; \
    install -m 0755 "/tmp/${release}/sccache" /usr/local/bin/sccache; \
    rm -rf /tmp/sccache.tar.gz "/tmp/${release}"

# cc follows RUSTC_WRAPPER; CMake launchers cover aws-lc-sys; sccache cannot cache incremental.
ENV RUSTC_WRAPPER=sccache \
    CMAKE_C_COMPILER_LAUNCHER=sccache \
    CMAKE_CXX_COMPILER_LAUNCHER=sccache \
    CARGO_INCREMENTAL=0 \
    SCCACHE_DIR=/var/cache/sccache
COPY --chmod=0755 docker/images/andromeda/with-sccache.sh /usr/local/bin/with-sccache

# GHA cache creds in CI; cache mount locally.
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo chef cook --release --locked --recipe-path recipe.json

# .git for shadow-rs.
COPY . .
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo build --release --locked --bin andromeda

FROM debian:trixie-20261027-slim AS runtime

# wget: compose healthchecks use it too.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system --gid 10001 andromeda \
    && useradd --system --uid 10001 --gid andromeda --home-dir /var/lib/andromeda --create-home andromeda \
    && mkdir -p /etc/andromeda \
    && chown -R andromeda:andromeda /etc/andromeda /var/lib/andromeda

COPY --from=builder /build/target/release/andromeda /usr/local/bin/andromeda

USER andromeda
WORKDIR /var/lib/andromeda

EXPOSE 1056

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD wget -qO- http://127.0.0.1:1056/ngsi-ld/v1/health >/dev/null 2>&1 || exit 1

LABEL org.opencontainers.image.title="Andromeda" \
      org.opencontainers.image.description="High-performance NGSI-LD context broker (ETSI GS CIM 009 v1.9.1)" \
      org.opencontainers.image.licenses="EUPL-1.2"

ENTRYPOINT ["andromeda"]
