# syntax=docker/dockerfile:1

# The container every tree runs in: the toolchain Andromeda pins, which the Rust base image's tag names, and
# cargo-nextest. A stack builds the `toolchain` stage from this directory; the repository is bind-mounted at /andromeda
# at run time, so the root rust-toolchain.toml, .cargo/config.toml and rustfmt.toml apply as they do on a developer
# machine, and the registry volume keeps dependencies between runs. CI builds the `suite` stage from a context holding the trees' inputs, which adds the test
# binaries of the suite and of the upgrade rehearsals as one nextest archive, so a run compiles nothing.
FROM rust:1.100.0-trixie@sha256:223cc32ddca8f3ee90c436dbc94b6b5f052ea7f9653c5201e9612f08ad7d21b8 AS toolchain

ARG NEXTEST_VERSION=0.9.149

RUN rustup component add clippy

RUN cargo install cargo-nextest --locked --version "${NEXTEST_VERSION}"

WORKDIR /andromeda/tools/andromeda-test-suite

FROM toolchain AS chef

RUN cargo install cargo-chef --locked --version 0.1.78

# The path the repository is mounted at in a stack, so the archive's paths are the ones a run sees.
WORKDIR /andromeda

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Test binaries carry no debug information and nothing compiles incrementally, so the archive stays small.
FROM chef AS builder

ENV CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_TEST_DEBUG=0

COPY rust-toolchain.toml ./
COPY .cargo ./.cargo

# The trees' third-party dependencies, a layer that stays while the manifests, the lockfile and the toolchain do.
COPY --from=planner /andromeda/recipe.json recipe.json
RUN cargo chef cook --tests --locked --package andromeda-test-suite --package andromeda-upgrade-tests --recipe-path recipe.json

COPY . .
# The target directory goes in the same layer, so the layer cache keeps the archive alone.
RUN cargo nextest archive --locked --package andromeda-test-suite --package andromeda-upgrade-tests \
    --archive-file /andromeda-test-suite.tar.zst \
    && rm -rf target

FROM toolchain AS suite

COPY --from=builder /andromeda-test-suite.tar.zst /andromeda-test-suite.tar.zst
