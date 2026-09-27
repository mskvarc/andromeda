//! The cryptographic provider every broker TLS configuration is built on.

use rustls::crypto::{CryptoProvider, aws_lc_rs::default_provider};

/// The `aws-lc-rs` provider the broker terminates TLS with.
///
/// Configurations are built against this explicit provider rather than the
/// process default, so they are unaffected by whichever provider another
/// workspace crate may have installed.
#[must_use]
pub fn broker_crypto_provider() -> CryptoProvider {
    default_provider()
}
