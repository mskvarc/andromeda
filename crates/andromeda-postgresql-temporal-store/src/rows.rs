//! Persistence-shaped row structs for temporal tables.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};

/// Result of a bulk TEA upsert: maps back to the input entry via its natural key.
#[derive(Debug)]
pub struct TeaBatchResultRow {
    /// Surrogate UUID assigned by the database.
    pub id: uuid::Uuid,
    /// Entity identifier (from the input).
    pub entity_id: Uri,
    /// Attribute IRI (from the input).
    pub attribute_name: AttributeName,
    /// Dataset ID (from the input).
    pub dataset_id: Option<Uri>,
}
