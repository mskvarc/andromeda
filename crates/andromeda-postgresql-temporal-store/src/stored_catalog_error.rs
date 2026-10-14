//! What a temporal catalogue row that no longer reads back as its domain value
//! looks like.
//!
//! The catalogue — `temporal_entities`, `temporal_attribute_streams` and the
//! name dictionaries it references — only ever holds identifiers, names and
//! keys the broker validated, so each variant is a column holding something the
//! broker never wrote. A column that cannot be read at all is a `sqlx` decode
//! failure and is reported as one, naming the column itself.

use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError};
use andromeda_postgresql_codes::{registry::UnknownCompactCode, stream_key::StreamKeyOutOfRange, temporal_entity_key::TemporalEntityKeyOutOfRange};
use thiserror::Error;

/// A stored temporal catalogue value that does not parse back into its domain
/// type.
#[derive(Debug, Error)]
pub enum StoredCatalogError {
    /// A `temporal_entities.id` is not a URI.
    #[error("a stored temporal_entities.id is not a URI: {0}")]
    EntityId(#[source] IdentifierParseError),
    /// A `temporal_entities.temporal_entity_key` is not a surrogate key.
    #[error("a stored temporal_entity_key is not a surrogate key: {0}")]
    EntityKey(#[source] TemporalEntityKeyOutOfRange),
    /// An entity type name the catalogue resolves is not an NGSI-LD name.
    #[error("a stored entity type name is not an NGSI-LD name: {0}")]
    EntityTypeName(#[source] NgsiLdReferenceError),
    /// An `attribute_names.name` is not an NGSI-LD name.
    #[error("a stored attribute_names.name is not an NGSI-LD name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// A recorded attribute family code is not one the registry assigned.
    #[error("a stored attribute_history attribute_kind is not a known code: {0}")]
    AttributeKind(#[source] UnknownCompactCode),
    /// A recorded value shape code is not one the registry assigned.
    #[error("a stored recorded shape is not a known code: {0}")]
    RecordedShape(#[source] UnknownCompactCode),
    /// A queued intent's `entry_needs` holds a code the registry never assigned.
    #[error("a stored temporal_append_intents.entry_needs code is not a known need: {0}")]
    IntentReach(#[source] UnknownCompactCode),
    /// A `temporal_attribute_streams.dataset_id` is not a URI.
    #[error("a stored temporal_attribute_streams.dataset_id is not a URI: {0}")]
    StreamDatasetId(#[source] IdentifierParseError),
    /// A `temporal_attribute_streams.stream_key` is not a surrogate key.
    #[error("a stored stream_key is not a surrogate key: {0}")]
    StreamKey(#[source] StreamKeyOutOfRange),
    /// A history count the store computed is below zero.
    #[error("a history count the store computed is {count}, below zero")]
    NegativeCount {
        /// The negative count the statement returned.
        count: i64,
    },
}
