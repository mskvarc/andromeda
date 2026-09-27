//! The broker's server-side TLS identity, shared by every listener that
//! terminates TLS, and its live reload.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2 requires the API to support HTTP over TLS.
//! The broker can terminate that TLS itself on its TCP listener, on its QUIC
//! (HTTP/3) listener and on its management listener, and all three present one
//! identity: TCP and QUIC share a host and port that `Alt-Svc` promises are the
//! same authority, so separate certificates would have no coherent use.
//!
//! The crate owns that identity end to end, one concern per module: the
//! [`config`]uration and its validated [`material_sources`], the
//! [`pem_material`] loaded from them, the [`crypto_provider`] every
//! configuration is built on, the [`handshake_policy`] a listener asks for, the
//! [`material_watch`] that reloads the files when they change on disk, and the
//! [`server_config_feed`] that turns each reload into a fresh rustls
//! configuration for one listener. Which ALPN tokens a listener speaks is the
//! listener crate's business; this crate knows none of them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod config;
pub mod crypto_provider;
pub mod handshake_policy;
pub mod material_sources;
pub mod material_watch;
pub mod pem_material;
pub mod server_config_feed;

#[cfg(test)]
mod handshake_probe;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration-scalars"],
    allow_workspace_dev_dependencies: ["andromeda-test-tls"],
    allow_production_dependencies: ["tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
