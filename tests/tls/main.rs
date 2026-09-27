//! TLS integration tests against live brokers, for both ways ETSI GS CIM 009
//! v1.9.1 clause 6.2 is met: the broker terminating TLS itself, and a reverse
//! proxy terminating it in front of a plaintext broker.
//!
//! These run against the stack in `docker/compose/test/tls/`, which publishes
//! three native-TLS brokers -- one with a TLS 1.2 floor, optional client
//! certificates and a TLS management listener, one requiring TLS 1.3 and a
//! client certificate, and one whose certificate the suite rotates -- and an
//! nginx terminator in front of a fourth, plaintext broker. Unlike the
//! `tls_terminator` regression tests, which assert the stack's files, these open
//! real TLS connections (see [`stack`]).
//!
//! The runner generates the certificate material and drives the stack:
//!
//! ```text
//! cargo xtask itest --stack tls
//! ```
//!
//! Every endpoint and the material directory are overridable through the
//! `ANDROMEDA_TEST_TLS_*` variables [`stack`] documents.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test setup aborts loudly on a missing live broker or certificate"
)]

mod certificate_rotation;
mod client_authentication;
mod management_listener;
mod minted_origin;
mod native_listener;
mod reverse_proxy;
mod stack;
