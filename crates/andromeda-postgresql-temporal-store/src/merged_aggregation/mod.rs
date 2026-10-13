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
    error::TemporalStoreError,
    intent_application::{intent_source::IntentSource, record_history_in_tx},
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
    temporal_entry_record::{TemporalEntryRecord, TemporalIntentRecord},
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_attribute_instance::{name_keys::MentionedNames, name_resolution::resolve_mentioned};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_temporal_query::history::build_temporal_history_query;
use andromeda_temporal::error::TemporalError;
use chrono::Utc;
use sea_query::{DeleteStatement, InsertStatement, PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection, Row, postgres::PgRow};
use std::collections::HashSet;

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
    ///
    /// The rows are recorded by the statement that records every history row,
    /// as the entries of one intent of the entity under a write of their own.
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
        let mut mentioned = MentionedNames::default();
        mentioned.attributes.extend(answered.iter().map(|row| row.attribute_name.clone())); // clone: the dictionary request owns the names it resolves
        let keys = resolve_mentioned(&mut *tx, mentioned, self.entity_type_names.as_ref(), self.attribute_names.as_ref()).await?;
        let entity_key = self.staged_entity(tx, entity_id, entity_types).await?;
        let streams = read_staged_streams(tx).await?;
        let missing: HashSet<(&AttributeName, Option<&Identifier>)> = answered
            .iter()
            .map(|row| (&row.attribute_name, row.dataset_id.as_ref()))
            .filter(|(name, dataset_id)| {
                !streams
                    .iter()
                    .any(|(staged, staged_dataset)| staged == *name && staged_dataset.as_ref() == *dataset_id)
            })
            .collect();
        for (position, (name, dataset_id)) in missing.into_iter().enumerate() {
            let name_key = keys
                .attribute(name)
                .ok_or_else(|| TemporalStoreInvariant::AttributeNameUnresolved(name.clone()))?; // clone: the invariant owns the name it failed on
            execute_insert(
                tx,
                stage_stream(staged_key(position.saturating_add(1)), entity_key, name_key.get(), dataset_id)?,
            )
            .await?;
        }
        let entries = answered
            .iter()
            .map(|row| TemporalEntryRecord::answered(row, &keys))
            .collect::<Result<Vec<_>, TemporalStoreError>>()?;
        let staged = WriteRecord::mint(Utc::now());
        let intents = [TemporalIntentRecord {
            entity_id: String::from(entity_id.as_str()),
            entity_type_name_keys: Vec::new(),
            base_ordinal: 0,
            entries,
        }];
        record_history_in_tx(
            tx,
            &IntentSource::Bound {
                record: &staged,
                intents: &intents,
            },
            &temporary_schema(),
        )
        .await?;
        execute_delete(tx, drop_restated_instances(staged.write_id().as_uuid())).await
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

/// The answered rows, an instance answered by several sources once.
fn distinct_instances(answered: &[TemporalHistoryRow]) -> Vec<&TemporalHistoryRow> {
    let mut seen = HashSet::new();
    answered
        .iter()
        .filter(|row| seen.insert((&row.attribute_name, &row.dataset_id, &row.instance)))
        .collect()
}

/// The grains the staging holds a stream of.
async fn read_staged_streams(tx: &mut PgConnection) -> Result<HashSet<Grain>, TemporalStoreError> {
    fetch(tx, staged_streams())
        .await?
        .iter()
        .map(|row| {
            let name = row
                .try_get::<String, _>(0)?
                .parse::<AttributeName>()
                .map_err(StoredCatalogError::AttributeName)?;
            let dataset_id = row
                .try_get::<Option<String>, _>(1)?
                .map(|raw| raw.parse::<Identifier>())
                .transpose()
                .map_err(StoredCatalogError::StreamDatasetId)?;
            Ok((name, dataset_id))
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
