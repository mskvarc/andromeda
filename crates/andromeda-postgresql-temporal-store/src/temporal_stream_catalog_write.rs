//! Establishing the `temporal_attribute_streams` half of the catalogue for one
//! chunk of writes.
//!
//! A stream names one attribute of one temporal entity under one `datasetId` and
//! hands out the narrow key every history row is stored against. This module
//! creates the streams a whole chunk of writes needs in one insert over every
//! entity's grains, which returns the key of every stream it created; only the
//! grains it did not create -- streams that already existed -- are read back,
//! in one more statement. A chunk of new entities therefore pays one statement,
//! and any chunk at most two. Neither statement references `entities`: clause
//! 5.6.6 leaves temporal evolution standing when current state is deleted, so
//! the stream catalogue has to be able to exist without it.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    resolved_evolutions::{EvolutionOrigin, ResolvedEvolutions},
    stream_grain_binds::{GrainColumns, GrainResolution, ResolvedStreams, StreamGrainBinds, StreamRow},
    temporal_store_invariant::TemporalStoreInvariant,
    temporal_stream::TemporalCatalogUpsert,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::temporal::TemporalAttributeStreams;
use andromeda_postgresql_seaquery::{
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Cond, Expr, ExprTrait, Iden, JoinType, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use std::collections::{HashMap, HashSet};

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `temporal_attribute_streams` alias the read-back resolves grains against.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The staging columns of one requested stream grain, carrying the narrow entity
/// key as an array column so one insert spans every entity in the chunk.
#[derive(Iden, Clone, Copy)]
enum StreamStaged {
    /// Narrow key of the temporal entity the stream belongs to.
    TemporalEntityKey,
    /// Dictionary identity of the stream's attribute name.
    AttributeNameKey,
    /// Multi-attribute discriminator of the stream.
    DatasetId,
}

/// The columns both catalogue statements return, in [`StreamRow`] order.
const RETURNED_COLUMNS: [TemporalAttributeStreams; 4] = [
    TemporalAttributeStreams::TemporalEntityKey,
    TemporalAttributeStreams::AttributeNameKey,
    TemporalAttributeStreams::DatasetId,
    TemporalAttributeStreams::StreamKey,
];

impl TemporalRepository {
    /// Establish the stream rows a whole chunk's history rows are stored against.
    ///
    /// `entity_keys` maps each chunk entity's identifier to the narrow key its
    /// temporal-entity row already carries, so the streams of every entity are
    /// inserted in one statement. Returns the stream each requested grain
    /// resolved to, keyed by the entity it belongs to, whether the chunk created
    /// it or found it already present.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when a name dictionary cannot
    /// resolve a name, [`TemporalStoreError::Invariant`] when the catalogue does
    /// not return a requested grain, [`TemporalStoreError::StoredCatalog`] when it returns
    /// one the chunk did not request, and [`TemporalStoreError::Database`] when
    /// a statement fails.
    pub(crate) async fn resolve_stream_catalog_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        upserts: &[TemporalCatalogUpsert],
        evolutions: &ResolvedEvolutions,
    ) -> Result<ResolvedStreams, TemporalStoreError> {
        let names = distinct_attribute_names(upserts);
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        // Resolved on this transaction's own connection: a stream row references
        // its attribute's dictionary key, so the key has to be part of the same
        // write, and taking a second pooled connection while holding this one
        // would put the write at the mercy of the pool's remaining capacity.
        let name_to_id = self.attribute_names.resolve_or_create(tx, &names).await?;
        let binds = StreamGrainBinds::new(upserts, evolutions, &name_to_id)?;
        if binds.is_empty() {
            return Ok(HashMap::new());
        }
        let mut resolution = GrainResolution::default();
        for origin in [EvolutionOrigin::Created, EvolutionOrigin::Existing] {
            let columns = binds.of(origin);
            if !columns.is_empty() {
                let created = fetch_streams(tx, stream_insert_batch_sql(target, origin)?, columns).await?;
                binds.resolve(&mut resolution, created)?;
            }
        }
        // The inserts return only the streams they created. The rest already
        // existed, or were created by a concurrent writer the insert waited on;
        // this read takes a fresh snapshot, so it sees both.
        let pending = binds.unresolved(&resolution);
        if !pending.is_empty() {
            let existing = fetch_streams(tx, stream_read_batch_sql(target), &pending).await?;
            binds.resolve(&mut resolution, existing)?;
        }
        if let Some(missing) = binds.first_missing(&resolution) {
            // clone: the invariant owns the attribute whose stream the catalogue failed to return.
            return Err(TemporalStoreInvariant::StreamUnresolved(missing.attribute_name.clone()).into());
        }
        Ok(resolution.into_streams())
    }
}

/// Run one catalogue statement over `columns` and collect the streams it returns.
async fn fetch_streams(tx: &mut Transaction<'_, Postgres>, sql: String, columns: &GrainColumns<'_>) -> Result<Vec<StreamRow>, TemporalStoreError> {
    Ok(sqlx::query_as(AssertSqlSafe(sql))
        .bind(&columns.temporal_entity_keys)
        .bind(&columns.attribute_name_keys)
        .bind(&columns.dataset_ids)
        .fetch_all(&mut **tx)
        .await?)
}

/// The distinct attribute names one chunk's stream upsert has to resolve.
fn distinct_attribute_names(upserts: &[TemporalCatalogUpsert]) -> Vec<AttributeName> {
    let mut seen = HashSet::new();
    upserts
        .iter()
        .flat_map(|upsert| upsert.streams.iter())
        .filter(|grain| seen.insert(grain.attribute_name.as_str()))
        .map(|grain| grain.attribute_name.clone()) // clone: dictionary resolution owns each distinct name.
        .collect()
}

/// The three staging columns of the stream-catalogue insert and read-back.
fn stream_staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(StreamStaged::TemporalEntityKey, PostgresType::BigintArray),
        UnnestColumn::new(StreamStaged::AttributeNameKey, PostgresType::IntegerArray),
        UnnestColumn::new(StreamStaged::DatasetId, PostgresType::TextArray),
    ]
}

/// The `UNNEST($1::bigint[], $2::integer[], $3::text[])` staging call the insert
/// and read-back share.
fn stream_staging_source(columns: &[UnnestColumn; 3]) -> sea_query::FunctionCall {
    unnest_call([
        (&columns[0], external_placeholder(1)),
        (&columns[1], external_placeholder(2)),
        (&columns[2], external_placeholder(3)),
    ])
}

/// Create the streams of one chunk's `origin` evolutions, and return the stored
/// grain and minted key of every stream it created.
///
/// `$1`/`$2`/`$3` are the requested-grain arrays: the entity key is an array
/// column, so one insert spans every entity. The stream key is generated by the
/// database, so `RETURNING` is where a new key is first learned.
///
/// An evolution that already existed may already own a requested stream, so
/// its insert arbitrates on the grain the catalogue is keyed by and leaves an
/// existing stream alone, returning nothing for it. An evolution this chunk
/// created owns none and no other transaction can give it one (see
/// [`ResolvedEvolutions`]), so its insert needs no arbitration: every grain it
/// binds is new.
fn stream_insert_batch_sql(target: &ContextTarget, origin: EvolutionOrigin) -> Result<String, TemporalStoreError> {
    let columns = stream_staging_columns();
    let call = stream_staging_source(&columns);
    let mut select = Query::select();
    select
        .expr(Expr::col((D, StreamStaged::TemporalEntityKey)))
        .expr(Expr::col((D, StreamStaged::AttributeNameKey)))
        .expr(Expr::col((D, StreamStaged::DatasetId)))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table_of(target, TemporalAttributeStreams::Table)).columns([
        TemporalAttributeStreams::TemporalEntityKey,
        TemporalAttributeStreams::AttributeNameKey,
        TemporalAttributeStreams::DatasetId,
    ]);
    insert
        .select_from(select)
        .map_err(TemporalStoreError::StatementBuild)?
        .returning(Query::returning().columns(RETURNED_COLUMNS));
    match origin {
        EvolutionOrigin::Created => {}
        EvolutionOrigin::Existing => {
            insert.on_conflict(
                OnConflict::columns([
                    TemporalAttributeStreams::TemporalEntityKey,
                    TemporalAttributeStreams::AttributeNameKey,
                    TemporalAttributeStreams::DatasetId,
                ])
                .do_nothing()
                .to_owned(),
            );
        }
    }
    let insert = insert;

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Read back the stream of every grain the insert did not create.
///
/// `$1`/`$2`/`$3` are the entity-key, attribute-name and dataset arrays of the
/// grains still unresolved; the `datasetId` match treats two absent identifiers
/// as one.
fn stream_read_batch_sql(target: &ContextTarget) -> String {
    let columns = stream_staging_columns();
    let call = stream_staging_source(&columns);

    let stream_match = Cond::all()
        .add(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((D, StreamStaged::TemporalEntityKey)))
        .add(Expr::col((S, TemporalAttributeStreams::AttributeNameKey)).equals((D, StreamStaged::AttributeNameKey)))
        .add(SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))).not_distinct_from(Expr::col((D, StreamStaged::DatasetId))));

    let mut select = Query::select();
    select.columns(RETURNED_COLUMNS.map(|column| (S, column))).from_function(call, D).join_as(
        JoinType::InnerJoin,
        table_of(target, TemporalAttributeStreams::Table),
        S,
        stream_match,
    );
    let select = select;

    let sql = select.to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &columns)
}

#[cfg(test)]
mod tests {
    use crate::{
        resolved_evolutions::EvolutionOrigin,
        temporal_stream_catalog_write::{stream_insert_batch_sql, stream_read_batch_sql},
    };
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn the_batched_stream_insert_carries_the_entity_key_as_an_array_column() {
        let sql = stream_insert_batch_sql(&ContextTarget::Live, EvolutionOrigin::Existing).expect("the batched stream insert builds");
        assert!(
            sql.contains(r#"INSERT INTO "temporal_attribute_streams" ("temporal_entity_key", "attribute_name_key", "dataset_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains("$1::bigint[]") && sql.contains("$2::integer[]") && sql.contains("$3::text[]"),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ON CONFLICT ("temporal_entity_key", "attribute_name_key", "dataset_id") DO NOTHING"#),
            "{sql}"
        );
        assert!(!sql.contains("uuid"), "{sql}");
    }

    /// The insert hands back the key of every stream it minted, so a chunk of
    /// new entities needs no read-back.
    #[test]
    fn the_batched_stream_insert_returns_the_grain_and_key_of_every_stream_it_creates() {
        let sql = stream_insert_batch_sql(&ContextTarget::Live, EvolutionOrigin::Existing).expect("the batched stream insert builds");
        assert!(
            sql.ends_with(r#"RETURNING "temporal_entity_key", "attribute_name_key", "dataset_id", "stream_key""#),
            "{sql}"
        );
    }

    /// An evolution the chunk created owns no stream, so its streams are
    /// inserted without conflict arbitration, and still returned.
    #[test]
    fn the_streams_of_a_created_evolution_are_inserted_without_arbitration() {
        let sql = stream_insert_batch_sql(&ContextTarget::Live, EvolutionOrigin::Created).expect("the batched stream insert builds");
        assert!(!sql.contains("ON CONFLICT"), "{sql}");
        assert!(
            sql.ends_with(r#"RETURNING "temporal_entity_key", "attribute_name_key", "dataset_id", "stream_key""#),
            "{sql}"
        );
    }

    /// The read-back answers in stored keys alone: the grain is recovered from
    /// the bind it answers for, so no dictionary join is needed.
    #[test]
    fn the_batched_read_back_resolves_null_dataset_grains_by_stored_keys_alone() {
        let sql = stream_read_batch_sql(&ContextTarget::Live);
        assert!(
            sql.starts_with(r#"SELECT "s"."temporal_entity_key", "s"."attribute_name_key", "s"."dataset_id", "s"."stream_key""#),
            "{sql}"
        );
        assert!(sql.contains("IS NOT DISTINCT FROM"), "{sql}");
        assert!(sql.contains("AS d(temporal_entity_key, attribute_name_key, dataset_id)"), "{sql}");
        assert!(!sql.contains("attribute_names"), "{sql}");
    }

    #[test]
    fn the_stream_catalogue_names_no_legacy_or_current_state_relation() {
        let insert = stream_insert_batch_sql(&ContextTarget::Live, EvolutionOrigin::Existing).expect("the batched stream insert builds");
        for statement in [insert, stream_read_batch_sql(&ContextTarget::Live)] {
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
            assert!(!statement.contains(r#""entities""#), "{statement}");
        }
    }
}
