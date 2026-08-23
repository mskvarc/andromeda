//! Establishing the `temporal_entities` half of the catalogue for one chunk of
//! writes.
//!
//! `temporal_entities` names the entity whose Temporal Evolution is being
//! recorded and carries the narrow key everything else references. This module
//! resolves that narrow key for every entity in a chunk in one round trip per
//! shape: the entities that declare types are upserted in one `UNNEST` insert
//! that harvests every key with `RETURNING`, and the entities that declare none
//! are looked up in one `id = ANY(...)` read. The table references `entities`
//! nowhere: clause 5.6.6 leaves temporal evolution standing when current state
//! is deleted, so the temporal catalogue has to be able to exist without it.

use crate::{error::TemporalStoreError, repo::TemporalRepository, temporal_stream::TemporalCatalogUpsert};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use andromeda_postgresql_query::entity_type_id::EntityTypeId;
use andromeda_postgresql_schema::temporal::TemporalEntities;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_array_elements_text,
    pg_array::{any, array_subquery},
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, OnConflict, PostgresQueryBuilder, Query};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use std::collections::HashMap;

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The staging columns of one temporal-entity upsert, named as the `AS d(…)`
/// alias spells them rather than as the columns they land in.
#[derive(Iden, Clone, Copy)]
enum EntityStaged {
    /// Entity identifier.
    Id,
    /// Write clock, read for both `created_at` and `modified_at`.
    RecordedAt,
    /// Type dictionary-key array as JSONB, expanded to `integer[]` in the insert.
    TypeIdsJson,
}

impl TemporalRepository {
    /// Resolve the narrow key of every entity in a chunk, creating the temporal
    /// entity for any that declare types and do not yet exist.
    ///
    /// An entity that carries types states what it is and may create its
    /// temporal-entity row; one that carries none — an attribute deletion, or a
    /// scope-only observation — can only extend a temporal entity that already
    /// exists, because clause 4.5.1 makes at least one type part of an entity's
    /// identity and nothing here may invent one. The typed entities are upserted
    /// in one statement that harvests their keys with `RETURNING`; the typeless
    /// ones are looked up in one `id = ANY(...)` read, and a requested identifier
    /// the read does not return has no evolution to extend.
    ///
    /// The chunk cannot contain two upserts for one entity: the upstream entity
    /// batch inserts current state with `ON CONFLICT` over the same chunk, so a
    /// duplicate identifier is already rejected before temporal recording runs.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when the type dictionary cannot
    /// resolve a name, [`TemporalStoreError::NotFound`] when a typeless entity
    /// has no temporal evolution to extend, [`TemporalStoreError::Decode`] when a
    /// harvested key is unreadable, and [`TemporalStoreError::Database`] when a
    /// statement fails.
    pub(crate) async fn upsert_temporal_entities_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        upserts: &[TemporalCatalogUpsert],
        recorded_at: DateTime<Utc>,
    ) -> Result<HashMap<Uri, TemporalEntityKey>, TemporalStoreError> {
        let mut entity_keys = HashMap::with_capacity(upserts.len());
        let typed: Vec<&TemporalCatalogUpsert> = upserts.iter().filter(|upsert| !upsert.types.is_empty()).collect();
        if !typed.is_empty() {
            let type_keys = self
                .entity_type_names
                .resolve_or_create(tx, &distinct_types(&typed))
                .await
                .map_err(|error| TemporalStoreError::NameLookup(error.to_string()))?;
            let binds = entity_upsert_binds(&typed, recorded_at, &type_keys)?;
            let sql = temporal_entity_upsert_batch_sql()?;
            let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(sql))
                .bind(&binds.ids)
                .bind(&binds.recorded_ats)
                .bind(&binds.type_ids_json)
                .bind(target.key().as_i64())
                .fetch_all(&mut **tx)
                .await?;
            harvest_keys(&mut entity_keys, rows)?;
        }
        let typeless_ids: Vec<&str> = upserts
            .iter()
            .filter(|upsert| upsert.types.is_empty())
            .map(|upsert| upsert.entity_id.as_str())
            .collect();
        if !typeless_ids.is_empty() {
            let sql = temporal_entity_keys_lookup_sql();
            let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(sql))
                .bind(&typeless_ids)
                .bind(target.key().as_i64())
                .fetch_all(&mut **tx)
                .await?;
            harvest_keys(&mut entity_keys, rows)?;
            if let Some(missing) = first_missing_typeless(upserts, &entity_keys) {
                return Err(TemporalStoreError::NotFound(format!(
                    "Temporal evolution of {missing} does not exist and the write declares no entity type to create it with"
                )));
            }
        }
        Ok(entity_keys)
    }
}

/// Record every harvested `(entity_key, id)` pair under its typed identifier.
fn harvest_keys(entity_keys: &mut HashMap<Uri, TemporalEntityKey>, rows: Vec<(i64, String)>) -> Result<(), TemporalStoreError> {
    for (stored, id) in rows {
        let key = TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
        let id = id
            .parse::<Uri>()
            .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.id '{id}': {error}")))?;
        entity_keys.insert(id, key);
    }
    Ok(())
}

/// The first typeless entity whose evolution the lookup did not resolve, if any.
///
/// A typeless upsert extends an evolution that must already exist, so a
/// requested identifier absent from the harvested keys is a `NotFound`.
fn first_missing_typeless<'a>(upserts: &'a [TemporalCatalogUpsert], entity_keys: &HashMap<Uri, TemporalEntityKey>) -> Option<&'a Uri> {
    upserts
        .iter()
        .filter(|upsert| upsert.types.is_empty())
        .map(|upsert| &upsert.entity_id)
        .find(|id| !entity_keys.contains_key(*id))
}

/// The distinct entity types one chunk's typed upserts have to resolve.
fn distinct_types(typed: &[&TemporalCatalogUpsert]) -> Vec<EntityTypeName> {
    let mut types: Vec<EntityTypeName> = typed
        .iter()
        .flat_map(|upsert| upsert.types.iter().cloned()) // clone: dictionary resolution owns each distinct name.
        .collect();
    types.sort_unstable();
    types.dedup();
    types
}

/// Column-major binds for one chunk's temporal-entity upsert.
struct EntityUpsertBinds<'a> {
    /// Identifier of each typed entity.
    ids: Vec<&'a str>,
    /// Write clock of each typed entity, one per entity so the array stays square.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Type dictionary keys of each typed entity, as a JSON array of integers.
    type_ids_json: Vec<Value>,
}

/// Spread every typed entity into column-major binds, one row per entity.
fn entity_upsert_binds<'a>(
    typed: &[&'a TemporalCatalogUpsert],
    recorded_at: DateTime<Utc>,
    type_keys: &HashMap<EntityTypeName, EntityTypeId>,
) -> Result<EntityUpsertBinds<'a>, TemporalStoreError> {
    let mut binds = EntityUpsertBinds {
        ids: Vec::with_capacity(typed.len()),
        recorded_ats: Vec::with_capacity(typed.len()),
        type_ids_json: Vec::with_capacity(typed.len()),
    };
    for upsert in typed {
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
        binds.ids.push(upsert.entity_id.as_str());
        binds.recorded_ats.push(recorded_at);
        binds.type_ids_json.push(Value::from(type_ids));
    }
    Ok(binds)
}

/// The three staging columns of the temporal-entity upsert, in bind order.
fn entity_staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(EntityStaged::Id, PostgresType::TextArray),
        UnnestColumn::new(EntityStaged::RecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(EntityStaged::TypeIdsJson, PostgresType::JsonbArray),
    ]
}

/// The temporal entities a chunk with declared types creates or refreshes.
///
/// `created_at` is set once and never moved: the conflict clause advances only
/// `modified_at` and the declared type keys, so the instant the Temporal
/// Evolution began survives every later write (clause 4.8). The write clock is
/// staged once per entity and read for both `created_at` and `modified_at`; the
/// type keys arrive as a JSONB array per entity and are expanded to `integer[]`
/// in the insert, the same interning `temporal_entities.type_ids` carries. Every
/// minted key is harvested with `RETURNING`, so no read-back is needed.
fn temporal_entity_upsert_batch_sql() -> Result<String, TemporalStoreError> {
    let columns = entity_staging_columns();
    let call = unnest_call([
        (&columns[0], external_placeholder(1)),
        (&columns[1], external_placeholder(2)),
        (&columns[2], external_placeholder(3)),
    ]);
    let type_ids = array_subquery(
        Query::select()
            .expr(cast(
                jsonb_array_elements_text(Expr::col((D, EntityStaged::TypeIdsJson))).into(),
                PostgresType::Integer,
            ))
            .to_owned(),
    );

    let mut select = Query::select();
    select
        .expr(external_placeholder(4))
        .expr(Expr::col((D, EntityStaged::Id)))
        .expr(Expr::col((D, EntityStaged::RecordedAt)))
        .expr(Expr::col((D, EntityStaged::RecordedAt)))
        .expr(type_ids)
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(TemporalEntities::Table).columns([
        TemporalEntities::ContextSpaceKey,
        TemporalEntities::Id,
        TemporalEntities::CreatedAt,
        TemporalEntities::ModifiedAt,
        TemporalEntities::TypeIds,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::columns([TemporalEntities::ContextSpaceKey, TemporalEntities::Id])
                .update_columns([TemporalEntities::ModifiedAt, TemporalEntities::TypeIds])
                .to_owned(),
        )
        .returning(Query::returning().columns([TemporalEntities::EntityKey, TemporalEntities::Id]));

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Read the narrow key of every typeless entity in one `id = ANY($1)` lookup.
///
/// `$1` is the requested identifiers as a `text[]`; a requested identifier the
/// read does not return has no temporal evolution to extend.
fn temporal_entity_keys_lookup_sql() -> String {
    Query::select()
        .columns([TemporalEntities::EntityKey, TemporalEntities::Id])
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::ContextSpaceKey).eq(external_placeholder(2)))
        .and_where(Expr::col(TemporalEntities::Id).eq(any(cast(external_placeholder(1), PostgresType::TextArray))))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal_entity_catalog_write::{entity_upsert_binds, first_missing_typeless, temporal_entity_keys_lookup_sql, temporal_entity_upsert_batch_sql},
        temporal_stream::TemporalCatalogUpsert,
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use andromeda_postgresql_query::entity_type_id::EntityTypeId;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

    fn typed(entity_id: &str, types: &[&str]) -> TemporalCatalogUpsert {
        TemporalCatalogUpsert {
            entity_id: entity_id.parse::<Uri>().expect("valid entity URI"),
            types: types.iter().map(|raw| raw.parse::<EntityTypeName>().expect("valid entity type")).collect(),
            streams: Vec::new(),
        }
    }

    fn typeless(entity_id: &str) -> TemporalCatalogUpsert {
        TemporalCatalogUpsert {
            entity_id: entity_id.parse::<Uri>().expect("valid entity URI"),
            types: Vec::new(),
            streams: Vec::new(),
        }
    }

    #[test]
    fn the_batched_entity_upsert_returns_every_chunk_key() {
        let sql = temporal_entity_upsert_batch_sql().expect("the batched entity upsert builds");
        assert!(sql.contains(r#"RETURNING "entity_key", "id""#), "{sql}");
        assert!(
            sql.contains("$1::text[]") && sql.contains("$2::timestamptz[]") && sql.contains("$3::jsonb[]") && !sql.contains("$4"),
            "{sql}"
        );
        assert!(sql.contains("AS d(id, recorded_at, type_ids_json)"), "{sql}");
    }

    #[test]
    fn the_batched_entity_upsert_expands_the_type_keys_to_an_integer_array() {
        let sql = temporal_entity_upsert_batch_sql().expect("the batched entity upsert builds");
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."type_ids_json")::integer)"#),
            "{sql}"
        );
        assert!(!sql.contains("text[]) AS \"type_ids\""), "{sql}");
    }

    #[test]
    fn a_repeated_write_never_moves_the_instant_the_evolution_began() {
        let sql = temporal_entity_upsert_batch_sql().expect("the batched entity upsert builds");
        assert!(
            sql.contains(r#"ON CONFLICT ("id") DO UPDATE SET "modified_at" = "excluded"."modified_at", "type_ids" = "excluded"."type_ids""#),
            "{sql}"
        );
        assert!(!sql.contains(r#""created_at" = "excluded""#), "{sql}");
    }

    #[test]
    fn the_typeless_lookup_matches_every_requested_identifier_in_one_array() {
        let sql = temporal_entity_keys_lookup_sql();
        assert!(
            sql.starts_with(r#"SELECT "entity_key", "id" FROM "temporal_entities" WHERE "id" = ANY($1::text[])"#),
            "{sql}"
        );
    }

    #[test]
    fn the_upsert_binds_only_the_typed_entities_and_squares_the_arrays() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let type_keys = HashMap::from([(VEHICLE.parse::<EntityTypeName>().expect("valid entity type"), EntityTypeId::from(4_i32))]);
        let typed = [typed("urn:ngsi-ld:Vehicle:1", &[VEHICLE]), typed("urn:ngsi-ld:Vehicle:2", &[VEHICLE])];
        let refs: Vec<&TemporalCatalogUpsert> = typed.iter().collect();
        let binds = entity_upsert_binds(&refs, recorded_at, &type_keys).expect("known types");
        pretty_assertions::assert_eq!(binds.ids, vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
        pretty_assertions::assert_eq!(binds.recorded_ats, vec![recorded_at, recorded_at]);
        pretty_assertions::assert_eq!(binds.type_ids_json.len(), 2);
        pretty_assertions::assert_eq!(binds.type_ids_json[0], serde_json::json!([4]));
    }

    #[test]
    fn a_typeless_intent_for_a_missing_evolution_is_reported_missing() {
        let upserts = [typed("urn:ngsi-ld:Vehicle:1", &[VEHICLE]), typeless("urn:ngsi-ld:Vehicle:2")];
        let resolved = HashMap::from([(
            "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity URI"),
            TemporalEntityKey::try_from(1_i64).expect("a positive narrow key"),
        )]);
        let missing = first_missing_typeless(&upserts, &resolved).expect("the typeless evolution is missing");
        pretty_assertions::assert_eq!(missing.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    #[test]
    fn a_typeless_intent_whose_evolution_exists_is_not_reported_missing() {
        let upserts = [typeless("urn:ngsi-ld:Vehicle:2")];
        let resolved = HashMap::from([(
            "urn:ngsi-ld:Vehicle:2".parse::<Uri>().expect("valid entity URI"),
            TemporalEntityKey::try_from(9_i64).expect("a positive narrow key"),
        )]);
        assert!(first_missing_typeless(&upserts, &resolved).is_none());
    }

    #[test]
    fn the_entity_catalogue_names_no_current_state_relation() {
        for statement in [
            temporal_entity_upsert_batch_sql().expect("the batched entity upsert builds"),
            temporal_entity_keys_lookup_sql(),
        ] {
            assert!(!statement.contains(r#""entities""#), "{statement}");
        }
    }
}
