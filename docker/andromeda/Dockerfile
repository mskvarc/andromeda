FROM rustlang/rust:nightly-alpine3.23 AS chef

RUN apk update && apk upgrade
RUN apk add --no-cache musl-dev git zlib-static zlib-dev

# Pin the nightly toolchain to a known-good date: the floating `nightly` the
# base image ships regressed an unrelated crate (`andromeda-jsonld`) with a
# "higher-ranked subtype error". This date matches the workspace's local build.
RUN rustup toolchain install nightly-2026-08-15 --profile minimal --component rustfmt \
    && rustup default nightly-2026-08-15

RUN cargo install cargo-chef

WORKDIR /build

FROM chef AS planner

COPY . .

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /build/recipe.json recipe.json

RUN cargo +nightly-2026-08-15 chef cook --recipe-path recipe.json

COPY . .

RUN cargo +nightly-2026-08-15 build --bin andromeda

FROM alpine:3.23

RUN apk add --no-cache bash ca-certificates

COPY --from=builder /build/target/debug/andromeda /usr/bin/andromeda

EXPOSE 1056

ENTRYPOINT ["andromeda"]
