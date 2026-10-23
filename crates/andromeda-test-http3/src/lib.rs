//! An HTTP/3 client for tests that issue real requests over QUIC.
//!
//! A listener serving HTTP/3 can only be exercised over the wire, through a
//! QUIC handshake and HTTP/3 request streams. This crate owns that client so
//! every crate testing an HTTP/3 listener dials it the same way: the
//! [`client`] connection and the responses it collects, and the [`error`]
//! dialing or exchanging can fail with.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod client;
pub mod error;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-test-tls"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["bytes", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
