//! HTTP/3 (QUIC) listener for the broker.
//!
//! This crate binds and serves the broker's HTTP/3 endpoint, the UDP-transport
//! sibling of the TCP listener in `andromeda-http-server`. HTTP/3 runs over QUIC
//! (RFC 9114), which terminates TLS 1.3 in process, so the listener is opt-in
//! and presents the broker's shared `[tls]` identity; it is disabled by default.
//!
//! The crate owns four concerns, one per module: the listener
//! [`config`]uration, the [`quic_crypto`] policy that asks the shared identity
//! for a QUIC-compatible configuration, the [`endpoint`] binding on the shared
//! port over UDP, and the [`execution`] loop that drives an already-assembled
//! `axum` router over accepted QUIC connections and installs rotated
//! certificates as they arrive. Advertising the endpoint to clients
//! (the `Alt-Svc` header on the TCP responses) and composing the two listeners
//! into one running broker are the composition layer's job, not this crate's.

pub mod config;
pub mod endpoint;
pub mod execution;
pub mod quic_crypto;

#[cfg(test)]
mod test_support;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration-scalars", "andromeda-server-tls"],
    allow_workspace_dev_dependencies: ["andromeda-test-tls"],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
