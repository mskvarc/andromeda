FROM rust:1.94.1-alpine3.23 AS builder

RUN apk add --no-cache musl-dev git zlib-static zlib-dev

WORKDIR /build
COPY . .

RUN cargo build

FROM alpine:3.23

RUN apk add --no-cache bash ca-certificates

COPY --from=builder /build/target/debug/andromeda /usr/local/bin/andromeda

EXPOSE 1056

ENTRYPOINT ["andromeda"]
