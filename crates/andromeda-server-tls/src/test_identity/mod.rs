//! Throwaway TLS identities for tests that complete real handshakes.
//!
//! A listener that terminates TLS cannot be exercised without a certificate,
//! and a test of client authentication or certificate rotation needs several:
//! an authority, the server leaves it issues, the client leaves it issues, and
//! files on disk a watcher can observe being replaced. The [`authority`]
//! issues identities, an [`identity`] is a leaf certificate and its key, the
//! [`identity_files`] are where a listener reads one from, and [`error`] is
//! what any of those steps can fail with.
//!
//! The module exists in this crate's own tests and under the `test-identity`
//! feature, which the crates testing a TLS listener enable for their tests
//! alone.

pub mod authority;
pub mod error;
pub mod identity;
pub mod identity_files;
