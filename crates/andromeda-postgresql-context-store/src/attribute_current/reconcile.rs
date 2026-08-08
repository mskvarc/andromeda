//! Refilling the projection from `attribute_history` on startup.

use crate::error::ContextStoreError;
use sqlx::PgPool;
use tracing::info;

/// Refill `attribute_current` from `attribute_history` for every TEA whose
/// latest observation is newer than the current row, or whose current row is
/// missing entirely. Returns the number of rows written.
///
/// This is a full pass over `attribute_history` with a per-TEA `DISTINCT ON`:
/// it is a startup one-shot whose cost scales with retention, not a steady-state
/// query. Making it incremental needs a durable "last applied" marker, which the
/// projection does not carry.
pub async fn reconcile_from_history(pool: &PgPool) -> Result<u64, ContextStoreError> {
    let result = sqlx::query(RECONCILE_FROM_HISTORY_SQL).execute(pool).await?;
    let rows = result.rows_affected();
    if rows > 0 {
        info!(rows, "attribute_current reconcile applied missing rows from attribute_history");
    }
    Ok(rows)
}

/// Reconcile statement run on startup.
///
/// Guarded on `COALESCE(recorded_at, time)` for the same reason the apply
/// statement is: `time` carries a client-supplied `observedAt` and does not
/// order writes.
const RECONCILE_FROM_HISTORY_SQL: &str = r#"
INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT DISTINCT ON (ah.tea_id)
    ah.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
    ah.value_number, ah.value_string, ah.value_bool, ah.value_geo,
    ah.value_object, ah.value_datetime, ah.value_uri, COALESCE(ah.recorded_at, ah.time)
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
LEFT JOIN attribute_current ac ON ac.tea_id = ah.tea_id
WHERE ac.tea_id IS NULL OR COALESCE(ah.recorded_at, ah.time) > ac.modified_at
ORDER BY ah.tea_id, COALESCE(ah.recorded_at, ah.time) DESC
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
WHERE attribute_current.modified_at <= EXCLUDED.modified_at
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reconcile_statement_compares_recorded_at_against_the_projection() {
        assert!(RECONCILE_FROM_HISTORY_SQL.contains("COALESCE(ah.recorded_at, ah.time) > ac.modified_at"));
        assert!(
            RECONCILE_FROM_HISTORY_SQL.contains("WHERE attribute_current.modified_at <= EXCLUDED.modified_at"),
            "the staleness guard must reject out-of-order applies"
        );
    }
}
