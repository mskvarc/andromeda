//! Throwaway TLS identities for tests that complete real handshakes.
//!
//! A listener that terminates TLS cannot be exercised without a certificate,
//! and a test of client authentication or certificate rotation needs several:
//! an authority, the server leaves it issues, the client leaves it issues, and
//! files on disk a watcher can observe being replaced. This crate owns that
//! setup so every crate testing a TLS listener generates it the same way: the
//! [`authority`] that issues identities, the [`identity`] a leaf certificate
//! and its key form, the [`identity_files`] a listener reads them from, and the
//! [`error`] any of those steps can fail with.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod authority;
pub mod error;
pub mod identity;
pub mod identity_files;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
