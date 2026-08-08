//! Rebuilding one TEA's `attribute_current` row from `attribute_history`.
//!
//! This is the context-store's own copy of the repair logic. The temporal
//! store keeps an identical version for its delete-attribute and
//! delete-instance paths; both stores are independent so neither imports
//! from the other.

use crate::error::ContextStoreError;
use sqlx::postgres::PgConnection;

/// Rebuild one TEA's `attribute_current` row from `attribute_history`.
///
/// `attribute_history` is the canonical record, so this recomputes the
/// projection rather than trusting any in-memory state. It is used from two
/// places: after deleting an attribute instance, where the surviving newest row
/// becomes current; and by the materializer when an apply fails, where it turns
/// a row the projection could not accept into a correct outcome instead of a
/// silent gap.
///
/// Both cases are covered without special-casing. If the owning TEA is gone the
/// refill selects nothing and the orphan delete removes any stale row, which is
/// the right answer. If the TEA is present the newest surviving observation is
/// restored.
pub async fn repair_attribute_current(conn: &mut PgConnection, tea_id: uuid::Uuid) -> Result<(), ContextStoreError> {
    sqlx::query(DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    sqlx::query(REMATERIALIZE_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    Ok(())
}

/// Drop the projection row for a TEA whose history is now empty.
const DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL: &str = r#"
DELETE FROM attribute_current ac
WHERE ac.tea_id = $1
  AND NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.tea_id = $1)
"#;

/// Rebuild the projection row for one TEA from its newest surviving history row.
///
/// The `ON CONFLICT` update is unconditional -- deliberately unlike the
/// materializer's staleness-guarded upsert. Deleting the newest instance makes
/// the correct current value *older* than the row already in the projection, so
/// a `modified_at <= EXCLUDED.modified_at` guard would reject exactly the write
/// that is needed here.
///
/// The `ORDER BY COALESCE(recorded_at, time) DESC` cannot use
/// `idx_attribute_history_tea_time`, so this costs one index probe per chunk
/// rather than a single seek. Acceptable on a single-instance delete, which is
/// not an ingest path; it does grow with retention.
const REMATERIALIZE_ATTRIBUTE_CURRENT_SQL: &str = r#"
INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT
    latest.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
    latest.value_number, latest.value_string, latest.value_bool, latest.value_geo,
    latest.value_object, latest.value_datetime, latest.value_uri,
    COALESCE(latest.recorded_at, latest.time)
FROM (
    SELECT * FROM attribute_history ah
    WHERE ah.tea_id = $1
    ORDER BY COALESCE(ah.recorded_at, ah.time) DESC
    LIMIT 1
) AS latest
JOIN temporal_entity_attributes tea ON tea.id = latest.tea_id
ON CONFLICT (tea_id) DO UPDATE SET
    entity_id         = EXCLUDED.entity_id,
    attribute_name_id = EXCLUDED.attribute_name_id,
    dataset_id        = EXCLUDED.dataset_id,
    value_type        = EXCLUDED.value_type,
    value_number      = EXCLUDED.value_number,
    value_string      = EXCLUDED.value_string,
    value_bool        = EXCLUDED.value_bool,
    value_geo         = EXCLUDED.value_geo,
    value_object      = EXCLUDED.value_object,
    value_datetime    = EXCLUDED.value_datetime,
    value_uri         = EXCLUDED.value_uri,
    modified_at       = EXCLUDED.modified_at
"#;
