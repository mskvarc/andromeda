//! The invariants Entity Map policy relies on, and how each one broke.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityMapIdParseError;
use andromeda_ngsi_ld_query::query_identity::QueryIdentityError;
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
    /// The identity of a query the broker already compiled cannot be written
    /// as bytes.
    #[error("the query identity cannot be written: {0}")]
    QueryIdentityUnwritable(#[source] QueryIdentityError),
}
