//! Persistence-shaped row structs for entity queries.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Entity data returned by `delete_batch` via the SQL `RETURNING` clause.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: String,
    /// The expanded JSON-LD payload at the time of deletion.
    pub payload: Value,
    /// Timestamp when the entity was originally created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last modification before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Result of an entity query containing matched rows and an optional total count.
#[derive(Debug)]
pub struct QueryResult {
    /// The matched entity payloads.
    pub rows: Vec<Value>,
    /// Total number of matching entities, present when a count was requested.
    pub total_count: Option<i64>,
}

/// Aggregated metadata for a single attribute across all entities.
#[derive(Debug)]
pub struct AttributeSummaryRow {
    /// The expanded IRI of the attribute.
    pub attr_iri: String,
    /// Number of distinct entities that carry this attribute.
    pub entity_count: i64,
    /// Entity type IRIs that contain this attribute.
    pub entity_types: Vec<String>,
    /// NGSI-LD attribute types observed for this attribute.
    pub attribute_types: Vec<String>,
}
