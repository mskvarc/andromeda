FROM rust:1.87-alpine3.22 AS builder

RUN apk add --no-cache musl-dev git

WORKDIR /build
COPY . .

RUN cargo build

FROM alpine:3.22

RUN apk add --no-cache ca-certificates

COPY --from=builder /build/target/debug/andromeda /usr/local/bin/andromeda

EXPOSE 1056

ENTRYPOINT ["andromeda"]
