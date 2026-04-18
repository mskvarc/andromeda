//! Persistence-shaped row structs for the `jsonld_contexts` table.

use crate::domain::jsonld_context::ContextKind;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Full metadata row (detailed projection).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContextMetadataRow {
    /// Internal identifier (URN/URL text).
    pub local_id: String,
    /// Public URL, if any.
    pub url: Option<String>,
    /// Stored kind.
    pub kind: ContextKind,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Expiry timestamp, if set.
    pub expires_at: Option<DateTime<Utc>>,
    /// Last access timestamp.
    pub last_usage: DateTime<Utc>,
    /// Access counter.
    pub number_of_hits: i64,
    /// Arbitrary extra info JSON column.
    pub extra_info: Option<Value>,
}

/// Summary row (id + url only).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContextSummaryRow {
    /// Internal identifier.
    pub id: String,
    /// Public URL, if any.
    pub url: Option<String>,
}
