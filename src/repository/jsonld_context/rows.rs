//! Persistence-shaped row structs for the `jsonld_contexts` table.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Full metadata row (detailed projection).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContextMetadataRow {
    /// Internal identifier (URN/URL text).
    pub local_id: String,
    /// Public URL, if any.
    pub url: Option<String>,
    /// Stored kind, as the raw `jsonld_context_kind` label. Projected via
    /// `kind::text`; the mapper turns it into the domain enum so an unknown
    /// label surfaces as a typed error rather than an sqlx decode failure.
    pub kind: String,
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
