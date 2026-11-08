//! Generic listener, connection execution and graceful HTTP draining, over
//! plaintext TCP or TLS terminated in process.

mod announced_close;
mod clocked_stream;
pub mod close_policy;
pub mod connection_bounds;
mod connection_builder;
mod connection_clock;
mod connection_lifetime;
pub mod error;
pub mod execution;
pub mod field_section;
mod http2_preface;
pub mod listener_security;
mod request_tracking;
mod staged_close_listener;
mod staged_close_stream;
mod tls_acceptor;
mod tls_tcp_listener;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration", "andromeda-server-tls"],
    allow_workspace_dev_dependencies: ["andromeda-server-tls"],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
