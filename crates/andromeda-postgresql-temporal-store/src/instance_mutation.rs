//! Mutation and retrieval of individual temporal attribute instances.

use crate::{
    attribute_instance::{collect_metadata, detect_attribute_type, extract_observed_at},
    attribute_value::{HistoryValueColumns, extract_attribute_value},
    error::TemporalStoreError,
    params::{HistoryValues, UpdateInstanceParams},
    projection_repair::repair_attribute_current,
    repo::TemporalRepository,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_id::InstanceId, uri::Uri};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;

impl TemporalRepository {
    /// Replace one expanded attribute instance through the typed PostgreSQL update path.
    pub(crate) async fn update_attribute_instance(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        instance_id: &InstanceId,
        instance: &Value,
    ) -> Result<(), TemporalStoreError> {
        let (owned, observed_at) = build_update_instance_state(instance)?;
        let params = build_update_instance_params(instance_id, entity_id, attribute_name, &owned, observed_at);
        self.update_instance(params).await
    }

    /// Delete a specific attribute instance and repair its current projection.
    pub(crate) async fn delete_instance(&self, instance_id: &InstanceId, entity_id: &Uri, attribute_name: &AttributeName) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query(
            r#"
            DELETE FROM attribute_history
            WHERE instance_id = $1
            AND tea_id IN (
                SELECT tea.id FROM temporal_entity_attributes tea
                JOIN attribute_names an ON an.id = tea.attribute_name_id
                WHERE tea.entity_id = $2 AND an.name = $3
            )
            RETURNING tea_id
            "#,
        )
        .bind(instance_id.as_str())
        .bind(entity_id.as_str())
        .bind(attribute_name.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = deleted else {
            return Err(TemporalStoreError::NotFound(format!("Attribute instance {instance_id} not found")));
        };
        let tea_id = row.try_get("tea_id")?;
        repair_attribute_current(&mut tx, tea_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Modify a specific attribute instance and repair its current projection.
    pub(crate) async fn update_instance(&self, params: UpdateInstanceParams<'_>) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(UPDATE_INSTANCE_SQL)
            .bind(params.instance_id.as_str())
            .bind(params.entity_id.as_str())
            .bind(params.attribute_name.as_str())
            .bind(params.values.value_number)
            .bind(params.values.value_string)
            .bind(params.values.value_bool)
            .bind(geometry_bytes(params.values.value_geo)?)
            .bind(params.values.value_object)
            .bind(params.values.value_datetime)
            .bind(params.values.value_uri.map(Uri::as_str))
            .bind(params.observed_at)
            .bind(params.values.metadata)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = updated else {
            return Err(TemporalStoreError::NotFound(format!("Attribute instance {} not found", params.instance_id)));
        };
        let tea_id = row.try_get("tea_id")?;
        repair_attribute_current(&mut tx, tea_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Fetch a single attribute instance for policy validation.
    pub(crate) async fn get_instance(
        &self,
        instance_id: &InstanceId,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<Option<sqlx::postgres::PgRow>, TemporalStoreError> {
        Ok(sqlx::query(
            r#"
            SELECT ah.*, an.name AS attribute_name, tea.attribute_type, tea.value_type
            FROM attribute_history ah
            JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE ah.instance_id = $1
            AND tea.entity_id = $2 AND an.name = $3
            "#,
        )
        .bind(instance_id.as_str())
        .bind(entity_id.as_str())
        .bind(attribute_name.as_str())
        .fetch_optional(&self.pool)
        .await?)
    }
}

/// Owned update values kept alive while PostgreSQL binds borrow them.
struct UpdateInstanceOwned {
    /// Typed history columns.
    columns: HistoryValueColumns,
    /// Expanded instance metadata.
    metadata: Option<Value>,
}

/// Decode one expanded instance into owned PostgreSQL update state.
fn build_update_instance_state(instance: &Value) -> Result<(UpdateInstanceOwned, Option<DateTime<Utc>>), TemporalStoreError> {
    let attribute_type = detect_attribute_type(instance);
    let attribute_value = extract_attribute_value(instance, attribute_type)?;
    let observed_at = extract_observed_at(instance)?;
    let metadata = collect_metadata(instance);
    Ok((
        UpdateInstanceOwned {
            columns: attribute_value.into_columns(),
            metadata,
        },
        observed_at,
    ))
}

/// Borrow owned update state into the repository parameter structure.
fn build_update_instance_params<'a>(
    instance_id: &'a InstanceId,
    entity_id: &'a Uri,
    attribute_name: &'a AttributeName,
    owner: &'a UpdateInstanceOwned,
    observed_at: Option<DateTime<Utc>>,
) -> UpdateInstanceParams<'a> {
    UpdateInstanceParams {
        instance_id,
        entity_id,
        attribute_name,
        values: HistoryValues {
            value_number: owner.columns.value_number,
            value_string: owner.columns.value_string.as_deref(),
            value_bool: owner.columns.value_bool,
            value_geo: owner.columns.value_geo.as_ref(),
            value_object: owner.columns.value_object.as_ref(),
            value_datetime: owner.columns.value_datetime,
            value_uri: owner.columns.value_uri.as_ref(),
            metadata: owner.metadata.as_ref(),
        },
        observed_at,
    }
}

const UPDATE_INSTANCE_SQL: &str = r#"
UPDATE attribute_history
SET value_number = $4, value_string = $5, value_bool = $6,
    value_geo = CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END,
    value_object = $8, value_datetime = $9, value_uri = $10,
    observed_at = $11, metadata = $12, modified_at = NOW()
WHERE instance_id = $1
AND tea_id IN (
    SELECT tea.id FROM temporal_entity_attributes tea
    JOIN attribute_names an ON an.id = tea.attribute_name_id
    WHERE tea.entity_id = $2 AND an.name = $3
)
RETURNING tea_id
"#;

#[cfg(test)]
mod tests {
    use crate::instance_mutation::UPDATE_INSTANCE_SQL;

    #[test]
    fn update_returns_the_owning_temporal_entity_attribute() {
        assert!(UPDATE_INSTANCE_SQL.contains("RETURNING tea_id"));
        assert!(!UPDATE_INSTANCE_SQL.contains("RETURNING instance_id"));
        assert!(UPDATE_INSTANCE_SQL.contains("modified_at = NOW()"));
    }
}
