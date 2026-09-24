//! HTTP/3 (QUIC) listener for the broker.
//!
//! This crate binds and serves the broker's HTTP/3 endpoint, the UDP-transport
//! sibling of the plaintext TCP listener in `andromeda-http-server`. HTTP/3 runs
//! over QUIC (RFC 9114), which terminates TLS 1.3 in process, so the listener is
//! opt-in and needs a certificate and key of its own; it is disabled by default.
//!
//! The crate owns four concerns, one per module: the listener
//! [`config`]uration, the [`tls`] assembly that turns a certificate pair into a
//! QUIC crypto configuration, the [`endpoint`] binding on the shared port over
//! UDP, and the [`execution`] loop that drives an already-assembled `axum`
//! router over accepted QUIC connections. Advertising the endpoint to clients
//! (the `Alt-Svc` header on the TCP responses) and composing the two listeners
//! into one running broker are the composition layer's job, not this crate's.

pub mod config;
pub mod endpoint;
pub mod execution;
pub mod tls;

#[cfg(test)]
mod test_support;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration-scalars"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
