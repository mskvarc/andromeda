//! Repair of the current-attribute projection from canonical history.

use crate::error::TemporalStoreError;
use sqlx::postgres::PgConnection;

/// Rebuild one temporal entity attribute's current projection from history.
pub async fn repair_attribute_current(conn: &mut PgConnection, tea_id: uuid::Uuid) -> Result<(), TemporalStoreError> {
    sqlx::query(DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    sqlx::query(REMATERIALIZE_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    Ok(())
}

const DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL: &str = r#"
DELETE FROM attribute_current ac
WHERE ac.tea_id = $1
  AND NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.tea_id = $1)
"#;

// An unconditional conflict update is required: deleting the newest history
// instance legitimately makes the correct projected value older.
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
