//! Persistence-shaped row structs for temporal tables.

/// Result of a bulk TEA upsert: maps back to the input entry via its natural key.
#[derive(Debug)]
pub struct TeaBatchResultRow {
    /// Surrogate UUID assigned by the database.
    pub id: uuid::Uuid,
    /// Entity identifier (from the input).
    pub entity_id: String,
    /// Attribute IRI (from the input).
    pub attribute_name: String,
    /// Dataset ID (from the input).
    pub dataset_id: Option<String>,
}
