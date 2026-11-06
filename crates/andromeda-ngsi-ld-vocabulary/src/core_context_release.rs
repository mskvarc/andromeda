//! One released NGSI-LD core `@context` document (clause 4.4) as the broker
//! embeds it: the URL ETSI serves it under, its bytes, and the SHA2-256 digest
//! of the bytes ETSI served when the copy was taken.
//!
//! ETSI publishes no digests and replaces a release in place, so the recorded
//! digest is the only record of which document the embedded copy is.

use iri_rs::Iri;

/// A core context release compiled into the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreContextRelease {
    /// The URL ETSI serves the release under.
    url: Iri<&'static str>,
    /// The release, byte for byte as ETSI serves it.
    document: &'static str,
    /// The SHA2-256 digest of the document ETSI serves under `url`.
    digest: [u8; 32],
}

impl CoreContextRelease {
    /// A release served under `url`, embedded as `document`, whose served bytes
    /// have the SHA2-256 digest `digest`.
    #[must_use]
    pub const fn new(url: Iri<&'static str>, document: &'static str, digest: [u8; 32]) -> CoreContextRelease {
        CoreContextRelease { url, document, digest }
    }

    /// The URL ETSI serves the release under.
    #[must_use]
    pub const fn url(&self) -> Iri<&'static str> {
        self.url
    }

    /// The embedded document.
    #[must_use]
    pub const fn document(&self) -> &'static str {
        self.document
    }

    /// The recorded SHA2-256 digest of the document ETSI serves.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}
