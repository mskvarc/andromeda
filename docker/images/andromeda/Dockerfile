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
    with-sccache cargo chef cook --profile dist --locked --recipe-path recipe.json

# .git for shadow-rs.
COPY . .
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo build --profile dist --locked --bin andromeda

# Owned by the broker's user in the runtime image, which has no shell to create them.
RUN mkdir -p /rootfs/etc/andromeda /rootfs/var/lib/andromeda

# glibc and libgcc_s are all the broker links dynamically. No shell, no package manager.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:cc66d222f0a7ec996b78a99fe76a969bc80b539a5016ad866c14d1949bc6d0ab AS runtime

COPY --from=builder --chown=10001:10001 /rootfs/ /
COPY --from=builder /build/target/dist/andromeda /usr/local/bin/andromeda

COPY LICENSE.md crates/andromeda-ngsi-ld-vocabulary/context/LICENSE-*.md crates/andromeda-http-headers/LICENSE-MIT.md /usr/share/doc/andromeda/

# 10001 has no passwd entry, so HOME is set here: the default configuration file and log directory resolve under it.
ENV HOME=/var/lib/andromeda
USER 10001:10001
WORKDIR /var/lib/andromeda

EXPOSE 1056

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["andromeda", "health"]

# The release build adds the labels that depend on the commit: source, revision, version, creation time and base image.
LABEL org.opencontainers.image.title="Andromeda" \
      org.opencontainers.image.description="NGSI-LD context broker written in Rust. Implements the ETSI GS CIM 009 v1.9.1 API, from entities and subscriptions to temporal history and federation, and checks every request against the semantics of the standard. Multi-tenant on PostgreSQL." \
      org.opencontainers.image.url="https://andromeda.skvarc.com" \
      org.opencontainers.image.documentation="https://andromeda.skvarc.com/docs/getting-started/installation/docker" \
      org.opencontainers.image.licenses="EUPL-1.2 AND MIT AND BSD-3-Clause AND W3C-20150513" \
      org.opencontainers.image.authors="Matej Škvarč <matej@skvarc.com>" \
      org.opencontainers.image.vendor="Matej Škvarč"

ENTRYPOINT ["andromeda"]
