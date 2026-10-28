//! Persistence-shaped row structs for the `jsonld_contexts` table.

use andromeda_json_literal::JsonLiteral;
use chrono::{DateTime, Utc};
use sqlx::types::Json;

/// Full metadata row (detailed projection).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContextMetadataRow {
    /// Internal identifier (URN/URL text).
    pub local_id: String,
    /// Public URL, if any.
    pub url: Option<String>,
    /// Stored kind, as the raw `JsonldContextKindCode` number; the mapper
    /// decodes it so an unknown number surfaces as a typed error rather than
    /// an sqlx decode failure.
    pub kind: i16,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Expiry timestamp, if set.
    pub expires_at: Option<DateTime<Utc>>,
    /// Last access timestamp.
    pub last_usage: DateTime<Utc>,
    /// Access counter.
    pub number_of_hits: i64,
}

/// Summary row (local identifier + url only).
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ContextSummaryRow {
    /// Internal identifier (URN/URL text).
    pub local_id: String,
    /// Public URL, if any.
    pub url: Option<String>,
}

/// The resolver's read of one unexpired row.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct FreshContextRow {
    /// The stored `@context` document.
    pub payload: Json<JsonLiteral>,
    /// When the row lapses, when it has an expiry.
    pub expires_at: Option<DateTime<Utc>>,
    /// The URL the row is keyed by, if any.
    pub url: Option<String>,
    /// Stored kind, as the raw `JsonldContextKindCode` number.
    pub kind: i16,
    /// The URL a download was finally read from; set exactly for a `Cached`
    /// row.
    pub document_url: Option<String>,
    /// The database clock the expiry was judged against.
    pub read_at: DateTime<Utc>,
}
