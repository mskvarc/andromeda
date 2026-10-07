//! HTTP/3 integration tests against a live broker serving native QUIC.
//!
//! These run against the `http3` stack, where one broker serves HTTP/3 over
//! QUIC on UDP:1056 alongside HTTP/1.1 and HTTP/2 on TCP:1056. They open a real
//! QUIC connection with an `h3` client (see [`client`]), so they exercise the
//! handshake and the transport themselves.
//!
//! The stack needs a self-signed `localhost` broker certificate, and the suite
//! runs with `--test-threads=1`.
//!
//! The suite reads `ANDROMEDA_TEST_HTTP3_AUTHORITY` (default `127.0.0.1:1056`),
//! `ANDROMEDA_TEST_HTTP3_CERT` (the path of the stack's broker certificate),
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
