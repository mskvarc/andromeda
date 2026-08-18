//! Establishing the temporal catalogue rows one write's history hangs off.
//!
//! The catalogue is two relations. `temporal_entities` names the entity whose
//! Temporal Evolution is being recorded and carries the narrow key everything
//! else references; `temporal_attribute_streams` names one attribute of that
//! entity under one `datasetId` and hands out the narrow key every history row
//! is stored against. Neither references `entities`: clause 5.6.6 leaves temporal
//! evolution standing when current state is deleted, so the temporal catalogue
//! has to be able to exist without it.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Cond, Expr, ExprTrait, Iden, JoinType, OnConflict, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
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

/// The staging columns of one requested stream grain.
#[derive(Iden, Clone, Copy)]
enum StreamStaged {
    /// Dictionary identity of the stream's attribute name.
    AttributeNameId,
    /// Multi-attribute discriminator of the stream.
    DatasetId,
}

impl TemporalRepository {
    /// Establish the catalogue rows one write's history rows are stored against.
    ///
    /// Returns the stream each requested grain resolved to, whether the write
    /// created it or found it already present.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when a name dictionary
    /// cannot resolve a name, [`TemporalStoreError::NotFound`] when
    /// the write declares no entity types and no temporal entity exists to
    /// extend, [`TemporalStoreError::Query`] when the catalogue does not return
    /// a requested grain, and [`TemporalStoreError::Database`] when a statement
    /// fails.
    pub async fn upsert_temporal_catalog_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        upsert: &TemporalCatalogUpsert,
        recorded_at: DateTime<Utc>,
    ) -> Result<HashMap<TemporalStreamKey, StreamKey>, TemporalStoreError> {
        let entity_key = self.upsert_temporal_entity(tx, upsert, recorded_at).await?;
        if upsert.streams.is_empty() {
            return Ok(HashMap::new());
        }
        // Resolved on this transaction's own connection: a stream row references
        // its attribute's dictionary key, so the key has to be part of the same
        // write, and taking a second pooled connection while holding this one
        // would put the write at the mercy of the pool's remaining capacity.
        let name_to_id = self
            .attribute_names
            .resolve_or_create(tx, &distinct_attribute_names(&upsert.streams))
            .await
            .map_err(|error| TemporalStoreError::NameLookup(error.to_string()))?;
        let binds = stream_binds(&upsert.streams, &name_to_id)?;
        // The data-modifying CTE the single statement used is rebuilt as an
        // insert followed by a read in the same transaction: the insert's rows
        // are visible to the read that follows it, so every requested grain —
        // freshly created or already present — resolves in one read without the
        // `RETURNING`-plus-`UNION ALL` the single command needed. The stream key
        // is minted by the database, so the read is also where a new key is
        // first learned.
        let insert_sql = stream_insert_sql()?;
        sqlx::query(AssertSqlSafe(insert_sql))
            .bind(entity_key.get())
            .bind(&binds.attribute_name_ids)
            .bind(&binds.dataset_ids)
            .execute(&mut **tx)
            .await?;
        let read_sql = stream_read_sql();
        let rows = sqlx::query(AssertSqlSafe(read_sql))
            .bind(entity_key.get())
            .bind(&binds.attribute_name_ids)
            .bind(&binds.dataset_ids)
            .fetch_all(&mut **tx)
            .await?;
        let resolved = rows.iter().map(decode_stream_row).collect::<Result<HashMap<_, _>, _>>()?;
        for key in &upsert.streams {
            if !resolved.contains_key(key) {
                return Err(TemporalStoreError::Query(format!(
                    "temporal catalogue upsert returned no stream for attribute '{}'",
                    key.attribute_name
                )));
            }
        }
        Ok(resolved)
    }
}

impl TemporalRepository {
    /// Create or refresh the temporal entity the write extends, and return its
    /// key.
    ///
    /// A write that carries entity types states what the entity is and may
    /// create it; a write that carries none — an attribute deletion, or a
    /// scope-only observation — can only extend a temporal entity that already
    /// exists, because clause 4.5.1 makes at least one type part of an entity's
    /// identity and nothing here may invent one. The types are stored as their
    /// dictionary keys, minted on this transaction's own connection for the
    /// same reason the stream's attribute keys are.
    async fn upsert_temporal_entity(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        upsert: &TemporalCatalogUpsert,
        recorded_at: DateTime<Utc>,
    ) -> Result<TemporalEntityKey, TemporalStoreError> {
        if !upsert.types.is_empty() {
            let type_keys = self
                .entity_type_names
                .resolve_or_create(tx, &upsert.types)
                .await
                .map_err(|error| TemporalStoreError::NameLookup(error.to_string()))?;
            let type_ids = upsert
                .types
                .iter()
                .map(|entity_type| {
                    type_keys
                        .get(entity_type)
                        .map(|key| key.get())
                        .ok_or_else(|| TemporalStoreError::NameLookup(format!("Entity type '{entity_type}' not found in dictionary after resolve")))
                })
                .collect::<Result<Vec<i32>, TemporalStoreError>>()?;
            let upsert_sql = temporal_entity_upsert_sql()?;
            sqlx::query(AssertSqlSafe(upsert_sql))
                .bind(upsert.entity_id.as_str())
                .bind(recorded_at)
                .bind(&type_ids)
                .execute(&mut **tx)
                .await?;
        }
        temporal_entity_key(tx, &upsert.entity_id).await
    }
}

/// Read back the narrow key of the temporal entity named by `entity_id`, which
/// must exist by now: either the write just created it or an earlier write did.
async fn temporal_entity_key(tx: &mut Transaction<'_, Postgres>, entity_id: &Uri) -> Result<TemporalEntityKey, TemporalStoreError> {
    let (key_sql, key_values) = temporal_entity_key_statement(entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
    let located = sqlx::query_with(AssertSqlSafe(key_sql), key_values).fetch_optional(&mut **tx).await?;
    let Some(row) = located else {
        return Err(TemporalStoreError::NotFound(format!(
            "Temporal evolution of {entity_id} does not exist and the write declares no entity type to create it with"
        )));
    };
    let stored: i64 = row
        .try_get("entity_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.entity_key: {error}")))?;
    TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))
}

/// Column-major binds for one stream-catalogue upsert.
struct StreamBinds<'a> {
    /// Dictionary identity of each stream's attribute name.
    attribute_name_ids: Vec<i32>,
    /// Multi-attribute discriminator of each stream.
    dataset_ids: Vec<Option<&'a str>>,
}

/// The distinct attribute names one catalogue upsert has to resolve.
fn distinct_attribute_names(streams: &[TemporalStreamKey]) -> Vec<AttributeName> {
    let mut seen = HashSet::with_capacity(streams.len());
    streams
        .iter()
        .filter(|key| seen.insert(key.attribute_name.as_str()))
        .map(|key| key.attribute_name.clone()) // clone: dictionary resolution owns each distinct name.
        .collect()
}

/// Spread the requested grains into column-major binds, one row per grain.
///
/// Repeated grains collapse to one row: one write may carry several
/// observations of one attribute, and they all extend the same stream.
fn stream_binds<'a>(streams: &'a [TemporalStreamKey], name_to_id: &HashMap<AttributeName, AttributeNameId>) -> Result<StreamBinds<'a>, TemporalStoreError> {
    let mut binds = StreamBinds {
        attribute_name_ids: Vec::with_capacity(streams.len()),
        dataset_ids: Vec::with_capacity(streams.len()),
    };
    let mut seen: HashSet<(AttributeNameId, Option<&'a str>)> = HashSet::with_capacity(streams.len());
    for key in streams {
        let attribute_name_id = *name_to_id
            .get(&key.attribute_name)
            .ok_or_else(|| TemporalStoreError::NameLookup(format!("Attribute name '{}' not found in dictionary after resolve", key.attribute_name)))?;
        let dataset_id = key.dataset_id.as_ref().map(Uri::as_str);
        if !seen.insert((attribute_name_id, dataset_id)) {
            continue;
        }
        binds.attribute_name_ids.push(attribute_name_id.get());
        binds.dataset_ids.push(dataset_id);
    }
    Ok(binds)
}

/// Read back one resolved stream and the grain it answers for.
fn decode_stream_row(row: &PgRow) -> Result<(TemporalStreamKey, StreamKey), TemporalStoreError> {
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
    Ok((TemporalStreamKey { attribute_name, dataset_id }, stream_key))
}

/// The temporal entity a write with declared types creates or refreshes.
///
/// `created_at` is set once and never moved: the conflict clause advances only
/// `modified_at` and the declared type keys, so the instant the Temporal
/// Evolution began survives every later write (clause 4.8). `$2` is bound once
/// and read for both `created_at` and `modified_at`; `$3` is the `integer[]`
/// of dictionary keys.
fn temporal_entity_upsert_sql() -> Result<String, TemporalStoreError> {
    let mut insert = Query::insert();
    insert.into_table(TemporalEntities::Table).columns([
        TemporalEntities::Id,
        TemporalEntities::CreatedAt,
        TemporalEntities::ModifiedAt,
        TemporalEntities::TypeIds,
    ]);
    insert
        .values([
            external_placeholder(1),
            external_placeholder(2),
            external_placeholder(2),
            cast(external_placeholder(3), PostgresType::IntegerArray),
        ])
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::column(TemporalEntities::Id)
                .update_columns([TemporalEntities::ModifiedAt, TemporalEntities::TypeIds])
                .to_owned(),
        );
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Read the narrow key of the temporal entity named by `id`.
fn temporal_entity_key_statement(id: &str) -> SelectStatement {
    Query::select()
        .column(TemporalEntities::EntityKey)
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::Id).eq(id))
        .to_owned()
}

/// The staging columns of the stream-catalogue insert.
fn stream_insert_columns() -> [UnnestColumn; 2] {
    [
        UnnestColumn::new(StreamStaged::AttributeNameId, PostgresType::IntegerArray),
        UnnestColumn::new(StreamStaged::DatasetId, PostgresType::TextArray),
    ]
}

/// Create the streams one write needs, leaving any that already exist.
///
/// `$1` is the narrow entity key, `$2`/`$3` the requested-grain arrays. The
/// conflict target is the grain the catalogue is keyed by; the stream key is
/// generated by the database and is not part of the insert.
fn stream_insert_sql() -> Result<String, TemporalStoreError> {
    let columns = stream_insert_columns();
    let call = unnest_call([(&columns[0], external_placeholder(2)), (&columns[1], external_placeholder(3))]);
    let mut select = Query::select();
    select
        .expr(external_placeholder(1))
        .expr(Expr::col((D, StreamStaged::AttributeNameId)))
        .expr(Expr::col((D, StreamStaged::DatasetId)))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(TemporalAttributeStreams::Table).columns([
        TemporalAttributeStreams::TemporalEntityKey,
        TemporalAttributeStreams::AttributeNameId,
        TemporalAttributeStreams::DatasetId,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::columns([
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
/// `$1` is the narrow entity key, `$2`/`$3` the requested attribute-name and
/// dataset arrays; the `datasetId` match treats two absent identifiers as one.
fn stream_read_sql() -> String {
    let columns = [
        UnnestColumn::new(StreamStaged::AttributeNameId, PostgresType::IntegerArray),
        UnnestColumn::new(StreamStaged::DatasetId, PostgresType::TextArray),
    ];
    let call = unnest_call([(&columns[0], external_placeholder(2)), (&columns[1], external_placeholder(3))]);

    let stream_match = Cond::all()
        .add(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).eq(external_placeholder(1)))
        .add(Expr::col((S, TemporalAttributeStreams::AttributeNameId)).equals((D, StreamStaged::AttributeNameId)))
        .add(SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))).not_distinct_from(Expr::col((D, StreamStaged::DatasetId))));

    let mut select = Query::select();
    select
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
        temporal_catalog_write::{distinct_attribute_names, stream_binds, stream_insert_sql, stream_read_sql, temporal_entity_upsert_sql},
        temporal_stream::TemporalStreamKey,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use std::collections::HashMap;

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn key(attribute_name: &str, dataset_id: Option<&str>) -> TemporalStreamKey {
        TemporalStreamKey {
            attribute_name: attribute_name.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: dataset_id.map(|raw| raw.parse::<Uri>().expect("valid dataset URI")),
        }
    }

    fn name_ids() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([
            (SPEED.parse().expect("valid speed name"), AttributeNameId::from(1_i32)),
            (BRAND.parse().expect("valid brand name"), AttributeNameId::from(2_i32)),
        ])
    }

    #[test]
    fn repeated_observations_of_one_grain_bind_a_single_stream_row() {
        let streams = [key(SPEED, None), key(SPEED, None), key(SPEED, None)];
        let binds = stream_binds(&streams, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32]);
        pretty_assertions::assert_eq!(binds.dataset_ids.len(), 1);
    }

    #[test]
    fn one_attribute_under_two_datasets_binds_two_streams() {
        let streams = [key(SPEED, None), key(SPEED, Some("urn:ngsi-ld:Dataset:a"))];
        let binds = stream_binds(&streams, &name_ids()).expect("known names");
        pretty_assertions::assert_eq!(binds.attribute_name_ids, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(binds.dataset_ids, vec![None, Some("urn:ngsi-ld:Dataset:a")]);
    }

    #[test]
    fn distinct_names_preserve_first_seen_order() {
        let streams = [key(BRAND, None), key(SPEED, None)];
        let names = distinct_attribute_names(&streams);
        pretty_assertions::assert_eq!(names.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec![BRAND, SPEED]);
    }

    #[test]
    fn the_stream_conflict_target_is_the_grain_the_catalogue_is_keyed_by() {
        let sql = stream_insert_sql().expect("the fixed stream insert builds");
        assert!(
            sql.contains(r#"ON CONFLICT ("temporal_entity_key", "attribute_name_id", "dataset_id") DO NOTHING"#),
            "{sql}"
        );
    }

    /// The stream key is generated by the database: the insert names the three
    /// grain columns only and binds two arrays beside the entity key.
    #[test]
    fn the_stream_insert_names_only_the_grain_and_lets_the_database_mint_the_key() {
        let sql = stream_insert_sql().expect("the fixed stream insert builds");
        assert!(
            sql.contains(r#"INSERT INTO "temporal_attribute_streams" ("temporal_entity_key", "attribute_name_id", "dataset_id")"#),
            "{sql}"
        );
        assert!(sql.contains("$2::integer[]") && sql.contains("$3::text[]") && !sql.contains("$4"), "{sql}");
        assert!(!sql.contains("stream_key") && !sql.contains("uuid"), "{sql}");
    }

    /// The read-back is where a freshly minted key is first learned, so it
    /// projects `stream_key` first.
    #[test]
    fn the_read_back_projects_the_generated_stream_key() {
        let sql = stream_read_sql();
        assert!(
            sql.starts_with(r#"SELECT "s"."stream_key", "an"."name" AS "attribute_name", "s"."dataset_id""#),
            "{sql}"
        );
    }

    #[test]
    fn a_repeated_write_never_moves_the_instant_the_evolution_began() {
        let sql = temporal_entity_upsert_sql().expect("the fixed entity upsert builds");
        assert!(
            sql.contains(r#"ON CONFLICT ("id") DO UPDATE SET "modified_at" = "excluded"."modified_at", "type_ids" = "excluded"."type_ids""#),
            "{sql}"
        );
        assert!(!sql.contains(r#""created_at" = "excluded""#), "{sql}");
    }

    /// The catalogue stores type dictionary keys, so the upsert binds an
    /// `integer[]` rather than the type IRIs.
    #[test]
    fn the_temporal_entity_upsert_binds_type_keys_as_an_integer_array() {
        let sql = temporal_entity_upsert_sql().expect("the fixed entity upsert builds");
        assert!(sql.contains("$3::integer[]"), "{sql}");
        assert!(!sql.contains("text[]"), "{sql}");
    }

    #[test]
    fn the_read_back_resolves_a_null_dataset_grain_to_a_null_one() {
        let sql = stream_read_sql();
        assert!(sql.contains("IS NOT DISTINCT FROM"), "{sql}");
        assert!(sql.contains(r#"AS "attribute_name""#), "{sql}");
        assert!(sql.contains("AS d(attribute_name_id, dataset_id)"), "{sql}");
    }

    #[test]
    fn the_catalogue_names_no_legacy_relation_and_no_current_state_table() {
        let insert = stream_insert_sql().expect("the fixed stream insert builds");
        for statement in [temporal_entity_upsert_sql().expect("the fixed entity upsert builds"), insert, stream_read_sql()] {
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
            assert!(!statement.contains(r#""entities""#), "{statement}");
        }
    }
}
