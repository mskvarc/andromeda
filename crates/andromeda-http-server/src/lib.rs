//! Generic listener, connection execution and graceful HTTP draining, over
//! plaintext TCP, TLS terminated in process, or a local socket.

mod accept_backoff;
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
pub mod local_socket_address;
pub mod local_socket_listener;
mod request_tracking;
mod staged_close_listener;
mod staged_close_stream;
mod tls_acceptor;
mod tls_tcp_listener;
mod unread_content;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration", "andromeda-server-tls"],
    allow_workspace_dev_dependencies: ["andromeda-server-tls"],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
