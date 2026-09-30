//! The invariants Context Registry policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::identifier::IdentifierParseError;
use iri_rs::InvalidIri;
use thiserror::Error;

/// A Context Registry invariant that did not hold.
///
/// None of these is the client's doing: each is a property of the embedded core
/// Context or of an identifier the registry already validated, so a broken one
/// is answered `InternalError`.
#[derive(Debug, Error)]
pub enum RegistryInvariant {
    /// The embedded core Context document is not JSON.
    #[error("the embedded core Context document is not JSON: {0}")]
    CoreContextDocument(#[source] serde_json::Error),
    /// The embedded core Context URL is not an identifier.
    #[error("the embedded core Context URL is not an identifier: {0}")]
    CoreContextIdentifier(#[source] IdentifierParseError),
    /// The embedded core Context identifier is not a URL.
    #[error("the embedded core Context identifier is not a URL")]
    CoreContextNotUrl,
    /// A Context identifier the registry already validated is not an IRI
    /// reference.
    #[error("a validated Context identifier is not an IRI reference: {0}")]
    IdentifierNotIri(#[source] InvalidIri<String>),
}
