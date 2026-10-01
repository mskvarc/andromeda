//! What an Entity row, or the discovery census kept beside the Entities, looks
//! like when it no longer reads back as its domain value.
//!
//! The broker only ever writes identifiers it validated and names it interned,
//! so each variant is a column holding something the broker never wrote.

use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError};
use andromeda_postgresql_codes::{entity_key::EntityKeyOutOfRange, registry::UnknownCompactCode};
use thiserror::Error;

/// A stored Entity or census column that does not parse back into its domain
/// type.
#[derive(Debug, Error)]
pub enum StoredEntityError {
    /// A stored entity identifier is not a URI.
    #[error("a stored entity identifier is not a URI: {0}")]
    Id(#[source] IdentifierParseError),
    /// A stored entity key is not a surrogate key, which the identity column's
    /// generated-always constraint makes impossible.
    #[error("a stored entity key is not a surrogate key: {0}")]
    Key(#[source] EntityKeyOutOfRange),
    /// A stored entity type IRI is not an NGSI-LD name the dictionary could
    /// have interned.
    #[error("a stored entity type name is not an NGSI-LD name: {0}")]
    TypeName(#[source] NgsiLdReferenceError),
    /// A stored attribute IRI is not an NGSI-LD name the dictionary could have
    /// interned.
    #[error("a stored attribute name is not an NGSI-LD name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// A current-state row or a census ledger records an attribute family
    /// under a code the registry never assigned.
    #[error("a stored attribute family is not a known code: {0}")]
    AttributeFamily(#[source] UnknownCompactCode),
    /// A census ledger sums below zero, which means it drifted from the
    /// Entities it counts.
    #[error("the discovery census sums to {sum}, below zero")]
    CensusCount {
        /// The negative sum the ledger holds.
        sum: i64,
    },
}
