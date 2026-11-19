//! The invariants registration policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::ContextSourceRegistrationIdParseError;
use andromeda_ngsi_ld_query::type_selection::type_selection_error::TypeSelectionError;
use iri_rs::InvalidIri;
use thiserror::Error;

/// A registration policy invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum RegistrationInvariant {
    /// A freshly minted registration identifier is not an identifier.
    #[error("a minted registration identifier is invalid: {0}")]
    MintedIdentifier(#[source] ContextSourceRegistrationIdParseError),
    /// A type selector the JSON-LD engine expanded has a leaf that is not a
    /// type name.
    #[error("an expanded entity type is not a type name: {0}")]
    ExpandedTypeName(#[source] TypeSelectionError),
    /// A registered `EntityInfo` names no entity type, which its wire form
    /// refuses.
    #[error("a registered entity selector names no type")]
    EntityInfoWithoutType,
    /// The entity store could not answer a proxied-registration admission check.
    #[error("the entity store could not answer a registration admission check")]
    EntityProbe,
    /// A declared attribute name handed to the entity store is not an IRI,
    /// which every expanded name is.
    #[error("a declared attribute name is not an IRI: {0}")]
    EntityProbeAttributeIri(#[source] InvalidIri<String>),
    /// The subscriptions forwarded under a registration could not be read before
    /// it was removed.
    #[error("the subscriptions forwarded under a registration could not be read")]
    ForwardedSubscriptionLookup,
}
