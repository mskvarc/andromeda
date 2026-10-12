//! Aggregating one entity over its local instances and the instances Context
//! Sources answered for the rest of it (clauses 4.5.19 and 5.7.4.4).
//!
//! Clause 5.7.4.4 renders an entity held in several parts with the "aggregated
//! values of Attribute instances" of the aggregated entity. The aggregation
//! semantics of clause 4.5.19 live in one statement, so the merged instances
//! are put where that statement reads: in one transaction, temporary copies of
//! the temporal catalogue, the stream catalogue and the history take the
//! entity's local rows and the answered ones, and the unchanged aggregation
//! statement runs against them. The copies drop when the transaction commits;
//! what lasts is only the dictionary names the answered attributes and types
//! were interned under, which the transaction commits so no cache holds a key
//! the database rolled back.

/// The statements that stage the history.
mod staging_statements;

use crate::{
    aggregation_rows_read::decode_aggregation_rows,
    attribute_history_write::insert_history_rows,
    error::TemporalStoreError,
    history_members::HistoryMembers,
    intent_sink::observation_row,
    merged_aggregation::staging_statements::{
        CopiedHistory,
        branch_schema,
        copy_entity,
        copy_history,
        copy_streams,
        drop_restated_instances,
        stage_entity,
        stage_stream,
        staged_entity_key,
        staged_streams,
        staging_tables,
        temporary_schema,
    },
    repo::TemporalRepository,
    stored_catalog_error::StoredCatalogError,
    temporal_store_invariant::TemporalStoreInvariant,
    unit_code_metadata::take_unit_code,
};
use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::stream_key::StreamKey;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::branch::qualified;
use andromeda_postgresql_temporal_query::history::build_temporal_history_query;
use andromeda_temporal::{append_intent::TemporalObservationIdentity, error::TemporalError};
use chrono::Utc;
use sea_query::{DeleteStatement, InsertStatement, PostgresQueryBuilder, SelectStatement, TableRef};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection, Row, postgres::PgRow};
use std::collections::{HashMap, HashSet};

/// The key the staging counts its own catalogue and stream keys down from:
/// far past every key an identity sequence assigns, so a staged key never
/// meets a copied one.
const STAGED_KEY_CEILING: i64 = i64::MAX;

/// One stream grain: an attribute and a `datasetId`.
type Grain = (AttributeName, Option<Identifier>);

impl TemporalRepository {
    /// Aggregate `entity_id` over its local instances together with
    /// `answered`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a staging statement fails, a dictionary
    /// name cannot be interned, or the aggregation refuses or does not read
    /// back.
    pub(crate) async fn merged_entity_aggregation(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        let mut tx = self.pool.begin().await.map_err(TemporalStoreError::from)?;
        let branch = branch_schema(schema_iden(target));
        for statement in staging_tables(&branch) {
            sqlx::query(AssertSqlSafe(statement))
                .execute(&mut *tx)
                .await
                .map_err(TemporalStoreError::from)?;
        }
        let copied = if compiled.representation.format.is_grouped() {
            CopiedHistory::Whole
        } else {
            CopiedHistory::Window {
                timeproperty: compiled.window.timeproperty,
                interval: compiled.window.interval,
            }
        };
        execute_insert(&mut tx, copy_entity(&branch, entity_id).map_err(TemporalStoreError::from)?).await?;
        execute_insert(&mut tx, copy_streams(&branch).map_err(TemporalStoreError::from)?).await?;
        execute_insert(&mut tx, copy_history(&branch, copied).map_err(TemporalStoreError::from)?).await?;
        self.stage_answered(&mut tx, entity_id, entity_types, answered).await?;
        let statement = build_temporal_history_query(Some(&temporary_schema()), entity_id, compiled);
        let pg_rows = fetch(&mut tx, statement).await?;
        let rows = decode_aggregation_rows(&pg_rows, entity_id, compiled)?;
        tx.commit().await.map_err(TemporalStoreError::from)?;
        Ok(rows)
    }

    /// Stage the answered rows beside the copied local ones, in streams of
    /// their own where the branch holds none of their grain, and drop the
    /// answered instances a local row already carries.
    async fn stage_answered(
        &self,
        tx: &mut PgConnection,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        answered: &[TemporalHistoryRow],
    ) -> Result<(), TemporalStoreError> {
        let answered = distinct_instances(answered);
        if answered.is_empty() {
            return Ok(());
        }
        let entity_key = self.staged_entity(tx, entity_id, entity_types).await?;
        let mut streams = read_staged_streams(tx).await?;
        let missing: Vec<Grain> = answered
            .iter()
            .map(|row| (row.attribute_name.clone(), row.dataset_id.clone())) // clone: the grain map owns its keys
            .filter(|grain| !streams.contains_key(grain))
            .collect::<HashSet<Grain>>()
            .into_iter()
            .collect();
        if !missing.is_empty() {
            let names: Vec<AttributeName> = missing.iter().map(|(name, _)| name.clone()).collect::<HashSet<_>>().into_iter().collect(); // clone: the dictionary resolves owned names
            let name_keys = self.attribute_names.resolve_or_create(&mut *tx, &names).await?;
            for (position, (name, dataset_id)) in missing.into_iter().enumerate() {
                let name_key = name_keys
                    .get(&name)
                    .ok_or_else(|| TemporalStoreInvariant::AttributeNameUnresolved(name.clone()))?; // clone: the invariant owns the name it failed on
                let stream_key = staged_key(position.saturating_add(1));
                execute_insert(tx, stage_stream(stream_key, entity_key, name_key.get(), dataset_id.as_ref())?).await?;
                streams.insert((name, dataset_id), StreamKey::try_from(stream_key).map_err(StoredCatalogError::StreamKey)?);
            }
        }
        let staged_write = WriteId::mint();
        let recorded_at = Utc::now();
        let rows = answered
            .iter()
            .enumerate()
            .map(|(position, row)| {
                let grain = (row.attribute_name.clone(), row.dataset_id.clone()); // clone: the grain map is keyed by owned grains
                let stream_key = *streams
                    .get(&grain)
                    .ok_or_else(|| TemporalStoreInvariant::StreamUnresolved(row.attribute_name.clone()))?; // clone: the invariant owns the name it failed on
                let ordinal = u32::try_from(position).map_err(TemporalStoreError::EntryPosition)?;
                // An aggregation reads the value and its unit alone (clause
                // 4.5.19), so an answered row is staged without the rest of
                // its metadata, whose names this tenant may never have keyed.
                // clone: the staged row owns its unit, the answer keeps its metadata.
                let (_, unit_code_id) = take_unit_code(row.metadata.clone())?;
                observation_row(
                    stream_key,
                    row,
                    recorded_at,
                    TemporalObservationIdentity {
                        write_id: staged_write,
                        ordinal: WriteOrdinal::at(ordinal),
                    },
                    HistoryMembers {
                        unit_code_id,
                        ..HistoryMembers::default()
                    },
                )
            })
            .collect::<Result<Vec<_>, TemporalStoreError>>()?;
        insert_history_rows(tx, staging_history_table(), rows).await?;
        execute_delete(tx, drop_restated_instances(staged_write.as_uuid())).await
    }

    /// The staged catalogue key of the entity: its copied local one, or one
    /// staged for an entity this branch holds none of.
    async fn staged_entity(&self, tx: &mut PgConnection, entity_id: &Identifier, entity_types: &[EntityTypeName]) -> Result<i64, TemporalStoreError> {
        if let Some(row) = fetch(tx, staged_entity_key()).await?.first() {
            return Ok(row.try_get::<i64, _>(0)?);
        }
        let type_keys = self.entity_type_names.resolve_or_create(&mut *tx, entity_types).await?;
        let type_keys: Vec<i32> = entity_types.iter().filter_map(|name| type_keys.get(name)).map(|key| key.get()).collect();
        let key = staged_key(0);
        execute_insert(tx, stage_entity(key, entity_id, type_keys, Utc::now())?).await?;
        Ok(key)
    }
}

/// The staged key `position` places below the ceiling.
fn staged_key(position: usize) -> i64 {
    STAGED_KEY_CEILING.saturating_sub(i64::try_from(position).unwrap_or(i64::MAX))
}

/// The temporary history relation the staged rows land in.
fn staging_history_table() -> TableRef {
    qualified(Some(&temporary_schema()), AttributeHistory::Table)
}

/// The answered rows, an instance answered by several sources once.
fn distinct_instances(answered: &[TemporalHistoryRow]) -> Vec<&TemporalHistoryRow> {
    let mut seen = HashSet::new();
    answered
        .iter()
        .filter(|row| seen.insert((&row.attribute_name, &row.dataset_id, &row.instance)))
        .collect()
}

/// The staged streams, keyed by grain.
async fn read_staged_streams(tx: &mut PgConnection) -> Result<HashMap<Grain, StreamKey>, TemporalStoreError> {
    fetch(tx, staged_streams())
        .await?
        .iter()
        .map(|row| {
            let stream_key = StreamKey::try_from(row.try_get::<i64, _>(0)?).map_err(StoredCatalogError::StreamKey)?;
            let name = row
                .try_get::<String, _>(1)?
                .parse::<AttributeName>()
                .map_err(StoredCatalogError::AttributeName)?;
            let dataset_id = row
                .try_get::<Option<String>, _>(2)?
                .map(|raw| raw.parse::<Identifier>())
                .transpose()
                .map_err(StoredCatalogError::StreamDatasetId)?;
            Ok(((name, dataset_id), stream_key))
        })
        .collect()
}

/// Run one select on the staging transaction.
async fn fetch(tx: &mut PgConnection, statement: SelectStatement) -> Result<Vec<PgRow>, TemporalStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(tx).await?)
}

/// Run one insert on the staging transaction.
async fn execute_insert(tx: &mut PgConnection, statement: InsertStatement) -> Result<(), TemporalStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(tx).await?;
    Ok(())
}

/// Run one delete on the staging transaction.
async fn execute_delete(tx: &mut PgConnection, statement: DeleteStatement) -> Result<(), TemporalStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(tx).await?;
    Ok(())
}
