//! Failures generating or writing a test identity.

use std::io;
use thiserror::Error;

/// A test identity could not be generated, trusted or written.
#[derive(Debug, Error)]
pub enum TestTlsError {
    /// Certificate or key generation failed.
    #[error("cannot generate test certificate material: {0}")]
    Generation(#[from] rcgen::Error),
    /// The authority certificate was not accepted as a trust anchor.
    #[error("cannot trust the test authority: {0}")]
    Trust(#[from] rustls::Error),
    /// Identity files could not be written.
    #[error("cannot write test identity files: {0}")]
    Io(#[from] io::Error),
}
