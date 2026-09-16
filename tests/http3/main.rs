//! HTTP/3 integration tests against a live broker serving native QUIC.
//!
//! These run against the stack in `docker/compose/test/http3/`, where one broker serves
//! HTTP/3 over QUIC on UDP:1056 alongside HTTP/1.1 and HTTP/2 on TCP:1056. They
//! open a real QUIC connection with an `h3` client (see [`client`]), so unlike
//! the TLS suite they exercise the handshake and the transport, not just the
//! repository files.
//!
//! Bring the stack up, run the suite, and tear it down:
//!
//! ```text
//! mkdir -p docker/compose/test/http3/certs
//! openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
//!   -keyout docker/compose/test/http3/certs/broker.key -out docker/compose/test/http3/certs/broker.crt \
//!   -subj "/CN=localhost" \
//!   -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
//!   -addext "basicConstraints=critical,CA:FALSE"
//! docker compose -f docker/compose/test/http3/docker-compose.yaml up -d --build --wait
//! cargo test --test http3 -- --test-threads=1
//! docker compose -f docker/compose/test/http3/docker-compose.yaml down --volumes --remove-orphans
//! ```
//!
//! The suite reads `ANDROMEDA_TEST_HTTP3_AUTHORITY` (default `127.0.0.1:1056`),
//! `ANDROMEDA_TEST_HTTP3_CERT` (default `docker/compose/test/http3/certs/broker.crt`),
//! `ANDROMEDA_TEST_HTTP3_SERVER_NAME` (default `localhost`), and
//! `ANDROMEDA_TEST_BROKER_URL` (default `http://127.0.0.1:1056/ngsi-ld/v1/`).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test setup aborts loudly on a missing live broker or certificate"
)]

mod advertisement;
mod client;
mod entity;
mod health;
mod normalization;
mod tls_trust;
