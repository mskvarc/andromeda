//! Generic listener, connection execution and graceful HTTP draining, over
//! plaintext TCP or TLS terminated in process.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod announced_close;
pub mod close_policy;
pub mod config;
mod connection_builder;
pub mod error;
pub mod execution;
pub mod field_section;
pub mod listener_security;
mod staged_close_listener;
mod staged_close_stream;
mod tls_acceptor;
mod tls_tcp_listener;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration-scalars", "andromeda-server-tls"],
    allow_workspace_dev_dependencies: ["andromeda-test-tls"],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
