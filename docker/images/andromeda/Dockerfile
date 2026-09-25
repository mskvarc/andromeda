# syntax=docker/dockerfile:1

# Debian-based, cargo-chef-cached, release build of the Andromeda broker.
# Stable base image only carries rustup; the actual compiler is the exact
# nightly pinned in rust-toolchain.toml, installed in the builder stage.
FROM rust:1.98.1-trixie AS chef

# aws-lc-rs (pulled in by rustls/quinn) compiles its C crypto library with
# cmake. Every other native build tool (cc, git, pkg-config) already ships in
# the buildpack-deps layer this image derives from.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

# Pin cargo-chef so the dependency-caching layer is reproducible.
RUN cargo install cargo-chef --locked --version 0.1.78

WORKDIR /build

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

# rust-toolchain.toml pins the exact nightly the workspace requires;
# .cargo/config.toml carries the -Zhigher-ranked-assumptions rustflag that both
# the dependency cook and the final build depend on. Both must be present before
# any cargo invocation so every compile uses the same toolchain and flags.
COPY rust-toolchain.toml ./
COPY .cargo ./.cargo
RUN rustup show

# sccache caches every compiled crate individually. The cargo-chef layer below
# is all-or-nothing: any Cargo.lock change invalidates it whole, and without
# sccache that recompiles every dependency. The prebuilt musl release is static,
# so one binary serves the Debian builder; its SHA-256 is pinned per architecture.
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

# rustc goes through sccache via RUSTC_WRAPPER. C compiles go through it too:
# the cc crate picks up an sccache RUSTC_WRAPPER on its own, and the launcher
# variables cover aws-lc-sys's cmake build. sccache cannot cache incremental
# compilation, so it stays off.
ENV RUSTC_WRAPPER=sccache \
    CMAKE_C_COMPILER_LAUNCHER=sccache \
    CMAKE_CXX_COMPILER_LAUNCHER=sccache \
    CARGO_INCREMENTAL=0 \
    SCCACHE_DIR=/var/cache/sccache
COPY --chmod=0755 docker/images/andromeda/with-sccache.sh /usr/local/bin/with-sccache

# Compile and cache only the dependency graph first: this layer is reused across
# source changes and only rebuilds when Cargo.lock or the recipe changes; sccache
# then limits that rebuild to the crates whose inputs actually changed. The two
# secrets are the GitHub Actions cache credentials CI passes in; a local build
# omits them and sccache uses the SCCACHE_DIR cache mount instead. Secret mounts
# never enter the layer or its cache key.
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo chef cook --release --locked --recipe-path recipe.json

# Build the broker itself. .git is present in the context so shadow-rs can embed
# the real commit metadata reported by the broker at runtime.
COPY . .
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo build --release --locked --bin andromeda

FROM debian:trixie-20260824-slim AS runtime

# ca-certificates for outbound TLS (notifications, context sources); wget backs
# the container healthcheck. wget (not curl) because the Compose stacks probe the
# broker with wget, so a single tool serves both the image and the stacks.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

# Unprivileged runtime user; /etc/andromeda is where the TLS key/cert and an
# optional andromeda.toml are mounted.
RUN groupadd --system --gid 10001 andromeda \
    && useradd --system --uid 10001 --gid andromeda --home-dir /var/lib/andromeda --create-home andromeda \
    && mkdir -p /etc/andromeda \
    && chown -R andromeda:andromeda /etc/andromeda /var/lib/andromeda

COPY --from=builder /build/target/release/andromeda /usr/local/bin/andromeda

USER andromeda
WORKDIR /var/lib/andromeda

# Broker listens on 0.0.0.0:1056 by default (andromeda-http-server ServerConfig).
EXPOSE 1056

# Health lives under the NGSI-LD API base, not the server root.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD wget -qO- http://127.0.0.1:1056/ngsi-ld/v1/health >/dev/null 2>&1 || exit 1

LABEL org.opencontainers.image.title="Andromeda" \
      org.opencontainers.image.description="High-performance NGSI-LD context broker (ETSI GS CIM 009 v1.9.1)" \
      org.opencontainers.image.licenses="EUPL-1.2"

ENTRYPOINT ["andromeda"]
