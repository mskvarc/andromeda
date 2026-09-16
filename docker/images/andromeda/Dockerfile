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

# Compile and cache only the dependency graph first: this layer is reused across
# source changes and only rebuilds when Cargo.lock or the recipe changes.
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

# Build the broker itself. .git is present in the context so shadow-rs can embed
# the real commit metadata reported by the broker at runtime.
COPY . .
RUN cargo build --release --locked --bin andromeda

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
