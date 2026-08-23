//! Establishing the `temporal_attribute_streams` half of the catalogue for one
//! chunk of writes.
//!
//! A stream names one attribute of one temporal entity under one `datasetId` and
//! hands out the narrow key every history row is stored against. This module
//! creates the streams a whole chunk of writes needs in one insert over every
//! entity's grains, then reads back the key each grain resolved to in one read,
//! so a batch of many entities pays two statements rather than two per entity.
//! Neither statement references `entities`: clause 5.6.6 leaves temporal
//! evolution standing when current state is deleted, so the stream catalogue has
//! to be able to exist without it.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{Cond, Expr, ExprTrait, Iden, JoinType, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use sqlx::{AssertSqlSafe, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::{HashMap, HashSet};

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `temporal_attribute_streams` alias the read-back resolves grains against.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `attribute_names` alias the read-back resolves each stream's IRI through.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct An;

/// The `attribute_name` output column of the read-back.
#[derive(Iden, Clone, Copy)]
#[iden = "attribute_name"]
struct AttributeNameColumn;

/// The staging columns of one requested stream grain, carrying the narrow entity
/// key as an array column so one insert spans every entity in the chunk.
#[derive(Iden, Clone, Copy)]
enum StreamStaged {
    /// Narrow key of the temporal entity the stream belongs to.
    TemporalEntityKey,
    /// Dictionary identity of the stream's attribute name.
    AttributeNameId,
    /// Multi-attribute discriminator of the stream.
    DatasetId,
}

impl TemporalRepository {
    /// Establish the stream rows a whole chunk's history rows are stored against.
    ///
    /// `entity_keys` maps each chunk entity's identifier to the narrow key its
    /// temporal-entity row already carries, so the streams of every entity are
    /// inserted and read back in one statement each. Returns the stream each
    /// requested grain resolved to, keyed by the entity it belongs to, whether
    /// the chunk created it or found it already present.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when a name dictionary cannot
    /// resolve a name, [`TemporalStoreError::Query`] when the catalogue does not
    /// return a requested grain, and [`TemporalStoreError::Database`] when a
    /// statement fails.
    pub(crate) async fn resolve_stream_catalog_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        upserts: &[TemporalCatalogUpsert],
        entity_keys: &HashMap<Uri, TemporalEntityKey>,
    ) -> Result<HashMap<(TemporalEntityKey, TemporalStreamKey), StreamKey>, TemporalStoreError> {
        let names = distinct_attribute_names(upserts);
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        // Resolved on this transaction's own connection: a stream row references
        // its attribute's dictionary key, so the key has to be part of the same
        // write, and taking a second pooled connection while holding this one
        // would put the write at the mercy of the pool's remaining capacity.
        let name_to_id = self
            .attribute_names
            .resolve_or_create(tx, &names)
            .await
            .map_err(|error| TemporalStoreError::NameLookup(error.to_string()))?;
        let binds = stream_binds(upserts, entity_keys, &name_to_id)?;
        if binds.temporal_entity_keys.is_empty() {
            return Ok(HashMap::new());
        }
        // The insert's rows are visible to the read that follows it in the same
        // transaction, so every requested grain — freshly created or already
        // present — resolves in one read. The stream key is minted by the
        // database, so the read is also where a new key is first learned.
        let insert_sql = stream_insert_batch_sql()?;
        sqlx::query(AssertSqlSafe(insert_sql))
            .bind(&binds.temporal_entity_keys)
            .bind(&binds.attribute_name_ids)
            .bind(&binds.dataset_ids)
            .bind(target.key().as_i64())
            .execute(&mut **tx)
            .await?;
        let read_sql = stream_read_batch_sql();
        let rows = sqlx::query(AssertSqlSafe(read_sql))
            .bind(&binds.temporal_entity_keys)
            .bind(&binds.attribute_name_ids)
            .bind(&binds.dataset_ids)
            .bind(target.key().as_i64())
            .fetch_all(&mut **tx)
            .await?;
        let resolved = rows.iter().map(decode_stream_row).collect::<Result<HashMap<_, _>, _>>()?;
        for upsert in upserts {
            let Some(entity_key) = entity_keys.get(&upsert.entity_id) else {
                continue;
            };
            for grain in &upsert.streams {
                if !resolved.contains_key(&(*entity_key, grain.clone())) {
                    return Err(TemporalStoreError::Query(format!(
                        "temporal catalogue upsert returned no stream for attribute '{}'",
                        grain.attribute_name
                    )));
                }
            }
        }
        Ok(resolved)
    }
}

/// Column-major binds for one chunk's stream-catalogue upsert.
struct StreamBinds<'a> {
    /// Narrow entity key each stream belongs to.
    temporal_entity_keys: Vec<i64>,
    /// Dictionary identity of each stream's attribute name.
    attribute_name_ids: Vec<i32>,
    /// Multi-attribute discriminator of each stream.
    dataset_ids: Vec<Option<&'a str>>,
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

/// Spread every entity's requested grains into column-major binds, one row per
/// grain, carrying the grain's entity key so one insert spans the whole chunk.
///
/// The dedup key is the full `(entity, attribute, datasetId)` grain: two
/// different entities restating one attribute yield two rows, while one entity
/// restating a grain yields one — a write may carry several observations of one
/// attribute, and they all extend the same stream.
fn stream_binds<'a>(
    upserts: &'a [TemporalCatalogUpsert],
    entity_keys: &HashMap<Uri, TemporalEntityKey>,
    name_to_id: &HashMap<AttributeName, AttributeNameId>,
) -> Result<StreamBinds<'a>, TemporalStoreError> {
    let mut binds = StreamBinds {
        temporal_entity_keys: Vec::new(),
        attribute_name_ids: Vec::new(),
        dataset_ids: Vec::new(),
    };
    let mut seen: HashSet<(i64, AttributeNameId, Option<&'a str>)> = HashSet::new();
    for upsert in upserts {
        // Every entity's key is resolved before this runs: a typeless intent for
        // a nonexistent evolution has already failed with `NotFound`, and a
        // typed one always comes back from the entity upsert's `RETURNING`.
        let Some(entity_key) = entity_keys.get(&upsert.entity_id) else {
            continue;
        };
        let entity_key = entity_key.get();
        for grain in &upsert.streams {
            let attribute_name_id = *name_to_id
                .get(&grain.attribute_name)
                .ok_or_else(|| TemporalStoreError::NameLookup(format!("Attribute name '{}' not found in dictionary after resolve", grain.attribute_name)))?;
            let dataset_id = grain.dataset_id.as_ref().map(Uri::as_str);
            if !seen.insert((entity_key, attribute_name_id, dataset_id)) {
                continue;
            }
            binds.temporal_entity_keys.push(entity_key);
            binds.attribute_name_ids.push(attribute_name_id.get());
            binds.dataset_ids.push(dataset_id);
        }
    }
    Ok(binds)
}

/// Read back one resolved stream, the entity it belongs to, and the grain it
/// answers for.
fn decode_stream_row(row: &PgRow) -> Result<((TemporalEntityKey, TemporalStreamKey), StreamKey), TemporalStoreError> {
    let stored_entity: i64 = row
        .try_get("temporal_entity_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.temporal_entity_key: {error}")))?;
    let entity_key = TemporalEntityKey::try_from(stored_entity).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    let raw_name: String = row
        .try_get("attribute_name")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_names.name: {error}")))?;
    let attribute_name = raw_name
        .parse::<AttributeName>()
        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_names.name '{raw_name}': {error}")))?;
    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id: {error}")))?
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id '{raw}': {error}")))
        })
        .transpose()?;
    let stored: i64 = row
        .try_get("stream_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.stream_key: {error}")))?;
    let stream_key = StreamKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    Ok(((entity_key, TemporalStreamKey { attribute_name, dataset_id }), stream_key))
}

/// The three staging columns of the stream-catalogue insert and read-back.
fn stream_staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(StreamStaged::TemporalEntityKey, PostgresType::BigintArray),
        UnnestColumn::new(StreamStaged::AttributeNameId, PostgresType::IntegerArray),
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

/// Create the streams a chunk needs, leaving any that already exist.
///
/// `$1`/`$2`/`$3` are the requested-grain arrays: the entity key is an array
/// column now, so one insert spans every entity. The conflict target is the
/// grain the catalogue is keyed by; the stream key is generated by the database
/// and is not part of the insert.
fn stream_insert_batch_sql() -> Result<String, TemporalStoreError> {
    let columns = stream_staging_columns();
    let call = stream_staging_source(&columns);
    let mut select = Query::select();
    select
        .expr(external_placeholder(4))
        .expr(Expr::col((D, StreamStaged::TemporalEntityKey)))
        .expr(Expr::col((D, StreamStaged::AttributeNameId)))
        .expr(Expr::col((D, StreamStaged::DatasetId)))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(TemporalAttributeStreams::Table).columns([
        TemporalAttributeStreams::ContextSpaceKey,
        TemporalAttributeStreams::TemporalEntityKey,
        TemporalAttributeStreams::AttributeNameId,
        TemporalAttributeStreams::DatasetId,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::columns([
                TemporalAttributeStreams::ContextSpaceKey,
                TemporalAttributeStreams::TemporalEntityKey,
                TemporalAttributeStreams::AttributeNameId,
                TemporalAttributeStreams::DatasetId,
            ])
            .do_nothing()
            .to_owned(),
        );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Read back the stream every requested grain resolves to, after the insert has
/// created any that were absent.
///
/// `$1`/`$2`/`$3` are the requested entity-key, attribute-name and dataset
/// arrays; the `datasetId` match treats two absent identifiers as one.
fn stream_read_batch_sql() -> String {
    let columns = stream_staging_columns();
    let call = stream_staging_source(&columns);

    let stream_match = Cond::all()
        .add(Expr::col((S, TemporalAttributeStreams::ContextSpaceKey)).eq(external_placeholder(4)))
        .add(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((D, StreamStaged::TemporalEntityKey)))
        .add(Expr::col((S, TemporalAttributeStreams::AttributeNameId)).equals((D, StreamStaged::AttributeNameId)))
        .add(SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))).not_distinct_from(Expr::col((D, StreamStaged::DatasetId))));

    let mut select = Query::select();
    select
        .expr(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)))
        .expr(Expr::col((S, TemporalAttributeStreams::StreamKey)))
        .expr_as(Expr::col((An, AttributeNames::Name)), AttributeNameColumn)
        .expr(Expr::col((S, TemporalAttributeStreams::DatasetId)))
        .from_function(call, D)
        .join_as(JoinType::InnerJoin, TemporalAttributeStreams::Table, S, stream_match)
        .join_as(
            JoinType::InnerJoin,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        );
    let select = select;

    let sql = select.to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &columns)
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
        temporal_stream_catalog_write::{stream_binds, stream_insert_batch_sql, stream_read_batch_sql},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use std::collections::HashMap;

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn grain(attribute_name: &str, dataset_id: Option<&str>) -> TemporalStreamKey {
        TemporalStreamKey {
            attribute_name: attribute_name.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: dataset_id.map(|raw| raw.parse::<Uri>().expect("valid dataset URI")),
        }
    }

    fn upsert(entity_id: &str, streams: Vec<TemporalStreamKey>) -> TemporalCatalogUpsert {
        TemporalCatalogUpsert {
            entity_id: entity_id.parse::<Uri>().expect("valid entity URI"),
            types: Vec::new(),
            streams,
        }
    }

    fn name_ids() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([
            (SPEED.parse().expect("valid speed name"), AttributeNameId::from(1_i32)),
            (BRAND.parse().expect("valid brand name"), AttributeNameId::from(2_i32)),
        ])
    }

    fn entity_keys(ids: &[(&str, i64)]) -> HashMap<Uri, TemporalEntityKey> {
        ids.iter()
            .map(|(id, key)| {
                (
                    id.parse::<Uri>().expect("valid entity URI"),
                    TemporalEntityKey::try_from(*key).expect("a positive narrow key"),
                )
            })
            .collect()
    }

    #[test]
    fn the_batched_stream_insert_carries_the_entity_key_as_an_array_column() {
        let sql = stream_insert_batch_sql().expect("the batched stream insert builds");
        assert!(
            sql.contains(r#"INSERT INTO "temporal_attribute_streams" ("temporal_entity_key", "attribute_name_id", "dataset_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains("$1::bigint[]") && sql.contains("$2::integer[]") && sql.contains("$3::text[]") && !sql.contains("$4"),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ON CONFLICT ("temporal_entity_key", "attribute_name_id", "dataset_id") DO NOTHING"#),
            "{sql}"
        );
        assert!(!sql.contains("stream_key") && !sql.contains("uuid"), "{sql}");
    }

    #[test]
    fn the_batched_read_back_resolves_null_dataset_grains_and_projects_the_key() {
        let sql = stream_read_batch_sql();
        assert!(
            sql.starts_with(r#"SELECT "s"."temporal_entity_key", "s"."stream_key", "an"."name" AS "attribute_name", "s"."dataset_id""#),
            "{sql}"
        );
        assert!(sql.contains("IS NOT DISTINCT FROM"), "{sql}");
        assert!(sql.contains("AS d(temporal_entity_key, attribute_name_id, dataset_id)"), "{sql}");
    }

    #[test]
    fn chunk_dedup_keeps_two_entities_sharing_one_attribute_as_two_streams() {
        let upserts = [
            upsert("urn:ngsi-ld:Vehicle:1", vec![grain(SPEED, None), grain(SPEED, None)]),
            upsert("urn:ngsi-ld:Vehicle:2", vec![grain(SPEED, None)]),
        ];
        let keys = entity_keys(&[("urn:ngsi-ld:Vehicle:1", 1), ("urn:ngsi-ld:Vehicle:2", 2)]);
        let binds = stream_binds(&upserts, &keys, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.temporal_entity_keys, vec![1_i64, 2_i64]);
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None, None]);
    }

    #[test]
    fn one_attribute_under_two_datasets_binds_two_streams_for_one_entity() {
        let upserts = [upsert(
            "urn:ngsi-ld:Vehicle:1",
            vec![grain(SPEED, None), grain(SPEED, Some("urn:ngsi-ld:Dataset:a"))],
        )];
        let keys = entity_keys(&[("urn:ngsi-ld:Vehicle:1", 7)]);
        let binds = stream_binds(&upserts, &keys, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.temporal_entity_keys, vec![7_i64, 7_i64]);
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None, Some("urn:ngsi-ld:Dataset:a")]);
    }

    #[test]
    fn the_stream_catalogue_names_no_legacy_or_current_state_relation() {
        let insert = stream_insert_batch_sql().expect("the batched stream insert builds");
        for statement in [insert, stream_read_batch_sql()] {
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
            assert!(!statement.contains(r#""entities""#), "{statement}");
        }
    }
}
