//! Atomic point-in-time copying of one Snapshot context branch into another.

use crate::snapshot::persistence_error;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_snapshot::error::SnapshotError;
use sqlx::{PgPool, Postgres, Transaction};

/// Copy every current and temporal relation in one repeatable-read transaction.
pub(crate) async fn promote_context_clone(pool: &PgPool, source_id: &Uri, target_id: &Uri) -> Result<(), SnapshotError> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let (source_key, target_key): (i64, i64) = sqlx::query_as(
        "SELECT source.context_space_key, target.context_space_key FROM snapshots source CROSS JOIN snapshots target WHERE source.id = $1 AND target.id = $2 FOR UPDATE OF target",
    )
    .bind(source_id.as_str())
    .bind(target_id.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;

    for statement in CLONE_STATEMENTS {
        sqlx::query(statement)
            .bind(source_key)
            .bind(target_key)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    }
    promote_control_row(&mut transaction, target_id).await?;
    transaction.commit().await.map_err(persistence_error)
}

const CLONE_STATEMENTS: [&str; 9] = [
    "INSERT INTO entities (context_space_key, created_at, expires_at, min_attribute_expires_at, id) SELECT $2, created_at, expires_at, min_attribute_expires_at, id FROM entities WHERE context_space_key = $1",
    "INSERT INTO entity_documents (context_space_key, entity_key, modified_at, payload) SELECT $2, target.entity_key, document.modified_at, document.payload FROM entity_documents document JOIN entities source ON source.context_space_key = $1 AND source.entity_key = document.entity_key JOIN entities target ON target.context_space_key = $2 AND target.id = source.id WHERE document.context_space_key = $1",
    "INSERT INTO entity_search (context_space_key, entity_key, location_expires_at, type_ids, attribute_name_ids, scope_paths, location, observation_space, operation_space) SELECT $2, target.entity_key, search.location_expires_at, search.type_ids, search.attribute_name_ids, search.scope_paths, search.location, search.observation_space, search.operation_space FROM entity_search search JOIN entities source ON source.context_space_key = $1 AND source.entity_key = search.entity_key JOIN entities target ON target.context_space_key = $2 AND target.id = source.id WHERE search.context_space_key = $1",
    "INSERT INTO current_attribute_values (context_space_key, entity_key, attribute_name_id, attribute_kind, value_kind, value_datetime, unit_code_id, value_bool, dataset_id, value_number, value_string, value_geo, value_object, value_uri) SELECT $2, target.entity_key, value.attribute_name_id, value.attribute_kind, value.value_kind, value.value_datetime, value.unit_code_id, value.value_bool, value.dataset_id, value.value_number, value.value_string, value.value_geo, value.value_object, value.value_uri FROM current_attribute_values value JOIN entities source ON source.context_space_key = $1 AND source.entity_key = value.entity_key JOIN entities target ON target.context_space_key = $2 AND target.id = source.id WHERE value.context_space_key = $1",
    "INSERT INTO temporal_entities (context_space_key, created_at, modified_at, id, type_ids) SELECT $2, created_at, modified_at, id, type_ids FROM temporal_entities WHERE context_space_key = $1",
    "INSERT INTO temporal_attribute_streams (context_space_key, temporal_entity_key, attribute_name_id, dataset_id) SELECT $2, target.entity_key, stream.attribute_name_id, stream.dataset_id FROM temporal_attribute_streams stream JOIN temporal_entities source ON source.context_space_key = $1 AND source.entity_key = stream.temporal_entity_key JOIN temporal_entities target ON target.context_space_key = $2 AND target.id = source.id WHERE stream.context_space_key = $1",
    "INSERT INTO attribute_history (recorded_at, context_space_key, write_id, stream_key, write_ordinal, attribute_kind, value_kind, observed_at, created_at, modified_at, deleted_at, value_datetime, generated_instance_uuid, unit_code_id, value_bool, verbatim_instance_id, value_number, value_string, value_geo, value_object, value_uri, metadata) SELECT history.recorded_at, $2, history.write_id, target_stream.stream_key, history.write_ordinal, history.attribute_kind, history.value_kind, history.observed_at, history.created_at, history.modified_at, history.deleted_at, history.value_datetime, history.generated_instance_uuid, history.unit_code_id, history.value_bool, history.verbatim_instance_id, history.value_number, history.value_string, history.value_geo, history.value_object, history.value_uri, history.metadata FROM attribute_history history JOIN temporal_attribute_streams source_stream ON source_stream.context_space_key = $1 AND source_stream.stream_key = history.stream_key JOIN temporal_entities source_entity ON source_entity.context_space_key = $1 AND source_entity.entity_key = source_stream.temporal_entity_key JOIN temporal_entities target_entity ON target_entity.context_space_key = $2 AND target_entity.id = source_entity.id JOIN temporal_attribute_streams target_stream ON target_stream.context_space_key = $2 AND target_stream.temporal_entity_key = target_entity.entity_key AND target_stream.attribute_name_id = source_stream.attribute_name_id AND target_stream.dataset_id IS NOT DISTINCT FROM source_stream.dataset_id WHERE history.context_space_key = $1",
    "INSERT INTO scope_history (recorded_at, context_space_key, temporal_entity_key, write_id, write_ordinal, value_kind, observed_at, created_at, modified_at, deleted_at, generated_instance_uuid, verbatim_instance_id, scope_path) SELECT history.recorded_at, $2, target.entity_key, history.write_id, history.write_ordinal, history.value_kind, history.observed_at, history.created_at, history.modified_at, history.deleted_at, history.generated_instance_uuid, history.verbatim_instance_id, history.scope_path FROM scope_history history JOIN temporal_entities source ON source.context_space_key = $1 AND source.entity_key = history.temporal_entity_key JOIN temporal_entities target ON target.context_space_key = $2 AND target.id = source.id WHERE history.context_space_key = $1",
    "INSERT INTO attribute_history_instance_claims (context_space_key, temporal_entity_key, attribute_name_id, verbatim_instance_id) SELECT $2, target.entity_key, claim.attribute_name_id, claim.verbatim_instance_id FROM attribute_history_instance_claims claim JOIN temporal_entities source ON source.context_space_key = $1 AND source.entity_key = claim.temporal_entity_key JOIN temporal_entities target ON target.context_space_key = $2 AND target.id = source.id WHERE claim.context_space_key = $1",
];

async fn promote_control_row(transaction: &mut Transaction<'_, Postgres>, target_id: &Uri) -> Result<(), SnapshotError> {
    let result = sqlx::query(
        "UPDATE snapshots SET status = 'success', modified_at = NOW(), claimed_by = NULL, claimed_at = NULL, lease_until = NULL WHERE id = $1 AND status = 'preparing'",
    )
    .bind(target_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if result.rows_affected() != 1 {
        return Err(SnapshotError::internal(format!("Clone target {target_id} is no longer preparing")));
    }
    Ok(())
}
