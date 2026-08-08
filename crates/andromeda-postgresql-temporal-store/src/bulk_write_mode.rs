//! Dispatch mode for bulk history writes.

/// Bulk-write execution mode for batch attribute-history INSERT paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BulkWriteMode {
    /// Classic `UNNEST($1::text[], $2::jsonb[], ...)` driver-side array path.
    /// One network round-trip per batch; server parses the array binds.
    #[default]
    Unnest,
    /// Binary `COPY ... FROM STDIN` into a transaction-scoped staging table,
    /// followed by `INSERT ... SELECT ... FROM stage`. Avoids the jsonb-array
    /// parse overhead and per-row deserialisation on the server.
    CopyStaging,
}
