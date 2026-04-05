FROM rustlang/rust:nightly-alpine3.23 AS chef

RUN apk update && apk upgrade
RUN apk add --no-cache musl-dev git zlib-static zlib-dev

RUN cargo install cargo-chef

WORKDIR /build

FROM chef AS planner

COPY . .

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /build/recipe.json recipe.json

RUN cargo +nightly chef cook --release --recipe-path recipe.json

COPY . .

RUN cargo +nightly build --release --bin andromeda

FROM alpine:3.23

RUN apk add --no-cache bash ca-certificates

COPY --from=builder /build/target/release/andromeda /usr/bin/andromeda

EXPOSE 1056

ENTRYPOINT ["andromeda"]
