# syntax=docker/dockerfile:1

# The container the suite runs in: the toolchain Andromeda pins and cargo-nextest. The repository is bind-mounted at
# /andromeda at run time, so the root rust-toolchain.toml, .cargo/config.toml and rustfmt.toml apply as they do on a
# developer machine, and the registry volume keeps dependencies between runs.
FROM rust:1.99.1-trixie

ARG TOOLCHAIN=nightly-2026-08-21
ARG NEXTEST_VERSION=0.9.149

RUN rustup toolchain install "${TOOLCHAIN}" --profile minimal --component clippy --component rustfmt \
    && rustup default "${TOOLCHAIN}"

RUN cargo install cargo-nextest --locked --version "${NEXTEST_VERSION}"

WORKDIR /andromeda/tools/andromeda-test-suite
