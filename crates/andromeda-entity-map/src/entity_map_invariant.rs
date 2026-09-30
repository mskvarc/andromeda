//! The invariants Entity Map policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityMapIdParseError;
use thiserror::Error;

/// An Entity Map policy invariant that did not hold.
///
/// None of these is the client's doing, so a broken one is answered
/// `InternalError`.
#[derive(Debug, Error)]
pub enum EntityMapInvariant {
    /// The map's lifetime carries its expiry outside the representable range.
    #[error("the entity map lifetime is outside the representable time range")]
    LifetimeOutOfRange,
    /// A freshly minted Entity Map identifier is not an identifier.
    #[error("a minted entity map identifier is invalid: {0}")]
    MintedIdentifier(#[source] EntityMapIdParseError),
    /// A geo-query the broker already parsed cannot be written back to JSON.
    #[error("the geo-query cannot be written back: {0}")]
    GeoQueryUnserializable(#[source] serde_json::Error),
}
