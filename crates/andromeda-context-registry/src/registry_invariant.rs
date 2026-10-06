//! The invariants Context Registry policy relies on, and how each one broke.

use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_lexicon::{
    identifier::IdentifierParseError,
    ngsi_ld_urn::{JsonLdContextId, JsonLdContextIdParseError},
};
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
    CoreContextDocument(#[source] JsonLiteralError),
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
    /// The identifier minted or derived for a new Context is not one.
    #[error("a minted Context identifier is not a JsonLdContext identifier: {0}")]
    MintedIdentifier(#[source] JsonLdContextIdParseError),
    /// A freshly minted random identifier is already held by a stored Context.
    #[error("a freshly minted Context identifier is already held: {identifier}")]
    MintedIdentifierTaken {
        /// The identifier that was already held.
        identifier: JsonLdContextId,
    },
    /// The memo of stored identifiers refused to forget a deleted one.
    #[error("the memo of stored Context identifiers cannot forget a deleted one: {0}")]
    MemoInvalidation(#[source] moka::PredicateError),
}
