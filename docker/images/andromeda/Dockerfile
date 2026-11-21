# syntax=docker/dockerfile:1

FROM rust:1.100.0-trixie@sha256:223cc32ddca8f3ee90c436dbc94b6b5f052ea7f9653c5201e9612f08ad7d21b8 AS chef

# cmake for aws-lc-sys.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef --locked --version 0.1.78

WORKDIR /build

# Must precede any cargo call in /build. The planner and the builder both inherit the pinned toolchain from this layer,
# so a source change never downloads it again.
COPY rust-toolchain.toml ./
COPY .cargo ./.cargo
RUN rustup show

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

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

# Embeds the dependency tree in the binary, where SBOM generators and scanners read the image's crates from.
ARG CARGO_AUDITABLE_VERSION=0.7.7
RUN set -eux; \
    arch="$(uname -m)"; \
    case "$arch" in \
        x86_64) sha256=7676443367c5e33d9fb22acef63cccb5e9202de12dae1b0579995eb3e2bc7a18 ;; \
        aarch64) sha256=c324af56990601354bbf40080f582ba46f8ec8b807a1388e25bb7e429d285c18 ;; \
        *) echo "no pinned cargo-auditable checksum for $arch" >&2; exit 1 ;; \
    esac; \
    release="cargo-auditable-${arch}-unknown-linux-musl"; \
    curl -fsSL -o /tmp/cargo-auditable.tar.xz \
        "https://github.com/rust-secure-code/cargo-auditable/releases/download/v${CARGO_AUDITABLE_VERSION}/${release}.tar.xz"; \
    echo "${sha256}  /tmp/cargo-auditable.tar.xz" | sha256sum -c -; \
    tar -xJf /tmp/cargo-auditable.tar.xz -C /tmp; \
    install -m 0755 "/tmp/${release}/cargo-auditable" /usr/local/bin/cargo-auditable; \
    rm -rf /tmp/cargo-auditable.tar.xz "/tmp/${release}"

# Writes the licence texts of the crates the binary links, which the image ships beside its own notice.
ARG CARGO_ABOUT_VERSION=0.9.3
RUN set -eux; \
    arch="$(uname -m)"; \
    case "$arch" in \
        x86_64) sha256=3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4 ;; \
        aarch64) sha256=eb4d9ff6ebd2518152a8308729431a04b3836e71d2bd5ead7aea69f91a71162e ;; \
        *) echo "no pinned cargo-about checksum for $arch" >&2; exit 1 ;; \
    esac; \
    release="cargo-about-${CARGO_ABOUT_VERSION}-${arch}-unknown-linux-musl"; \
    curl -fsSL -o /tmp/cargo-about.tar.gz \
        "https://github.com/EmbarkStudios/cargo-about/releases/download/${CARGO_ABOUT_VERSION}/${release}.tar.gz"; \
    echo "${sha256}  /tmp/cargo-about.tar.gz" | sha256sum -c -; \
    tar -xzf /tmp/cargo-about.tar.gz -C /tmp; \
    install -m 0755 "/tmp/${release}/cargo-about" /usr/local/bin/cargo-about; \
    rm -rf /tmp/cargo-about.tar.gz "/tmp/${release}"

# cc follows RUSTC_WRAPPER; CMake launchers cover aws-lc-sys; sccache cannot cache incremental. cargo auditable runs as
# the workspace wrapper inside sccache, and sccache caches a wrapped compile only when the wrapper's first argument is
# the bare name rustc, which cargo passes only when RUSTC names it so.
ENV RUSTC_WRAPPER=sccache \
    RUSTC=rustc \
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

# cargo-about reads the metadata of every locked package, every platform's included, while the build downloads only the
# packages of its own.
RUN cargo fetch --locked

COPY . .
# The binary moves out and the target directory goes in the same layer, so the layer cache keeps the binary alone.
RUN --mount=type=secret,id=ACTIONS_RESULTS_URL,env=ACTIONS_RESULTS_URL \
    --mount=type=secret,id=ACTIONS_RUNTIME_TOKEN,env=ACTIONS_RUNTIME_TOKEN \
    --mount=type=cache,id=andromeda-sccache,target=/var/cache/sccache,sharing=locked \
    with-sccache cargo auditable build --profile dist --locked --bin andromeda \
    && install -D -m 0755 target/dist/andromeda /dist/andromeda \
    && rm -rf target

# The licence of every crate the binary links, for the platform it is built for, read offline from the sources the build
# fetched. cargo-about collects no NOTICE file, which Apache-2.0 asks to be kept with the binary, so each one a linked
# crate ships is appended.
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN set -eu; \
    target="$(uname -m)-unknown-linux-gnu"; \
    cargo about generate --frozen --fail --target "$target" \
        --output-file /dist/third-party-licenses.md docker/images/andromeda/third-party-licenses.hbs; \
    cargo tree --frozen --package andromeda --edges normal --target "$target" --prefix none --format '{p}' \
        | cut -d ' ' -f 1,2 | sort -u \
        | while read -r name version; do \
            for notice in "${CARGO_HOME}"/registry/src/*/"${name}-${version#v}"/NOTICE*; do \
                [ -f "$notice" ] || continue; \
                printf '### %s %s\n\n~~~text\n' "$name" "${version#v}"; \
                cat "$notice"; \
                printf '\n~~~\n\n'; \
            done; \
        done >> /dist/third-party-licenses.md

# Owned by the broker's user in the runtime image, which has no shell to create them.
RUN mkdir -p /rootfs/etc/andromeda /rootfs/var/lib/andromeda

# glibc and libgcc_s are all the broker links dynamically. No shell, no package manager.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:cb87acc574abe719b6e5c547384defa2f7b1f24af9605d3f618cf852495d8be2 AS runtime

COPY --from=builder --chown=10001:10001 /rootfs/ /
COPY --from=builder /dist/andromeda /usr/local/bin/andromeda
COPY --from=builder /dist/third-party-licenses.md /usr/share/doc/andromeda/

COPY LICENSE.md NOTICE.md crates/andromeda-ngsi-ld-vocabulary/context/LICENSE-*.md crates/andromeda-http-headers/LICENSE-MIT.md /usr/share/doc/andromeda/

# 10001 has no passwd entry, so HOME is set here: the default configuration file and log directory resolve under it.
ENV HOME=/var/lib/andromeda
USER 10001:10001
WORKDIR /var/lib/andromeda

EXPOSE 1056

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["andromeda", "health"]

# The CI build adds the source and base image labels. The image carries no commit, so the revision, version and creation
# time go on the index that a commit's or a release's tag points at.
LABEL org.opencontainers.image.title="Andromeda" \
      org.opencontainers.image.description="NGSI-LD context broker written in Rust. Implements the ETSI GS CIM 009 v1.9.1 API, from entities and subscriptions to temporal history and federation, and checks every request against the semantics of the standard. Multi-tenant on PostgreSQL." \
      org.opencontainers.image.url="https://andromeda.skvarc.com" \
      org.opencontainers.image.documentation="https://andromeda.skvarc.com/docs/getting-started/installation/docker" \
      org.opencontainers.image.licenses="EUPL-1.2" \
      org.opencontainers.image.authors="Matej Škvarč <matej@skvarc.com>" \
      org.opencontainers.image.vendor="Matej Škvarč"

ENTRYPOINT ["andromeda"]
