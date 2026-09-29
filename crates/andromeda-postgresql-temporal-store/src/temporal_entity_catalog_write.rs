//! Establishing the `temporal_entities` half of the catalogue for one chunk of
//! writes.
//!
//! `temporal_entities` names the entity whose Temporal Evolution is being
//! recorded and carries the narrow key everything else references. This module
//! resolves that narrow key for every entity in a chunk in one round trip per
//! shape: the entities whose write may bring an evolution into being are
//! upserted in one `UNNEST` insert that harvests every key with `RETURNING`,
//! and the rest are looked up in one `id = ANY(...)` read. The table references
//! `entities` nowhere: clause 5.6.6 leaves temporal evolution standing when
//! current state is deleted, so the temporal catalogue has to be able to exist
//! without it.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    resolved_evolutions::{EvolutionOrigin, ResolvedEvolutions},
    temporal_stream::TemporalCatalogUpsert,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_postgresql_schema::temporal::TemporalEntities;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_array_elements_text,
    pg_array::{any, array_subquery},
    pg_type::PostgresType,
    placeholder::external_placeholder,
    returning_image::Old,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_temporal::write_purpose::TemporalWritePurpose;
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
    EntityTypeNameKeysJson,
}

impl TemporalRepository {
    /// Resolve the narrow key of every entity in a chunk, creating the temporal
    /// entity for any whose write may bring one into being.
    ///
    /// The chunk splits in two by [`evolution_resolution`]. The upserts that may
    /// create go into one statement that harvests their keys with `RETURNING`;
    /// the rest are looked up in one `id = ANY(...)` read, and a requested
    /// identifier the read does not return has no evolution.
    ///
    /// What that absence means is the upsert's
    /// [`purpose`](TemporalCatalogUpsert::purpose) to say, not this method's.
    /// Clause 5.6.16 deletes a Temporal Evolution on its own and clause 5.6.6
    /// deletes current state on its own, so a client can reach "the entity
    /// exists, its evolution does not" in either order: a
    /// [`Deletion`](TemporalWritePurpose::Deletion) that meets it has no
    /// instance left to end and resolves to no key, while an
    /// [`Observation`](TemporalWritePurpose::Observation) that meets it and
    /// states no type has nowhere to record and fails the chunk.
    ///
    /// The chunk cannot contain two upserts for one entity: the upstream entity
    /// batch inserts current state with `ON CONFLICT` over the same chunk, so a
    /// duplicate identifier is already rejected before temporal recording runs.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when the type dictionary cannot
    /// resolve a name, [`TemporalStoreError::NotFound`] when a typeless
    /// observation has no temporal evolution to extend,
    /// [`TemporalStoreError::Decode`] when a harvested key is unreadable, and
    /// [`TemporalStoreError::Database`] when a statement fails.
    pub(crate) async fn upsert_temporal_entities_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        upserts: &[TemporalCatalogUpsert],
        recorded_at: DateTime<Utc>,
    ) -> Result<ResolvedEvolutions, TemporalStoreError> {
        let mut resolved = ResolvedEvolutions::default();
        let (creating, resolving): (Vec<&TemporalCatalogUpsert>, Vec<&TemporalCatalogUpsert>) =
            upserts.iter().partition(|upsert| match evolution_resolution(upsert) {
                EvolutionResolution::Create => true,
                EvolutionResolution::Lookup => false,
            });
        if !creating.is_empty() {
            let type_keys = self
                .entity_type_names
                .resolve_or_create(tx, &distinct_types(&creating))
                .await
                .map_err(|error| TemporalStoreError::NameLookup(error.to_string()))?;
            let binds = entity_upsert_binds(&creating, recorded_at, &type_keys)?;
            let sql = temporal_entity_upsert_batch_sql(target)?;
            let rows: Vec<(i64, String, bool)> = sqlx::query_as(AssertSqlSafe(sql))
                .bind(&binds.ids)
                .bind(&binds.recorded_ats)
                .bind(&binds.entity_type_name_keys_json)
                .fetch_all(&mut **tx)
                .await?;
            for (stored, id, inserted) in rows {
                harvest_key(&mut resolved, stored, &id, origin_of(inserted))?;
            }
        }
        if !resolving.is_empty() {
            let ids: Vec<&str> = resolving.iter().map(|upsert| upsert.entity_id.as_str()).collect();
            let sql = temporal_entity_keys_lookup_sql(target);
            let rows: Vec<(i64, String)> = sqlx::query_as(AssertSqlSafe(sql)).bind(&ids).fetch_all(&mut **tx).await?;
            for (stored, id) in rows {
                harvest_key(&mut resolved, stored, &id, EvolutionOrigin::Existing)?;
            }
            if let Some(missing) = first_missing_typeless_observation(&resolving, resolved.keys()) {
                return Err(TemporalStoreError::NotFound(format!(
                    "Temporal evolution of {missing} does not exist and the write declares no entity type to create it with"
                )));
            }
        }
        Ok(resolved)
    }
}

/// The origin the upsert's `old.temporal_entity_key IS NULL` column reports.
const fn origin_of(inserted: bool) -> EvolutionOrigin {
    if inserted { EvolutionOrigin::Created } else { EvolutionOrigin::Existing }
}

/// Whether resolving one upsert's narrow key may create the temporal entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvolutionResolution {
    /// The write states what the entity is, so a missing evolution is created.
    Create,
    /// The write may only find an evolution that is already there.
    Lookup,
}

/// What resolving one upsert's narrow key is allowed to do to the catalogue.
///
/// An observation extends the Temporal Evolution and therefore creates it when
/// it is not there, provided the write says what the entity is: clause 4.5.6
/// makes the evolution "the Entity [...] as mandated by clause 4.5.1", and
/// clause 4.5.1 makes at least one type part of that, so an observation
/// declaring none can only extend one that already exists and nothing here may
/// invent a type it never wrote.
///
/// A deletion never creates one, whatever it declares. Clauses 4.5.7 and 4.5.8
/// record a deletion as one more instance of a sequence — "In case the Property
/// is deleted, an instance of the Property is recorded with its value set to the
/// URI `urn:ngsi-ld:null` and the deletedAt Temporal Property set" — so an
/// evolution clause 5.6.16 already removed holds no sequence for the ending to
/// belong to. Bringing one into being out of an ending would assert that
/// instances had existed and stopped, which is precisely the history the client
/// deleted.
const fn evolution_resolution(upsert: &TemporalCatalogUpsert) -> EvolutionResolution {
    match upsert.purpose {
        TemporalWritePurpose::Observation if upsert.types.is_empty() => EvolutionResolution::Lookup,
        TemporalWritePurpose::Observation => EvolutionResolution::Create,
        TemporalWritePurpose::Deletion => EvolutionResolution::Lookup,
    }
}

/// Record one harvested `(entity_key, id)` pair under its typed identifier.
fn harvest_key(resolved: &mut ResolvedEvolutions, stored: i64, id: &str, origin: EvolutionOrigin) -> Result<(), TemporalStoreError> {
    let key = TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    let id = id
        .parse::<Uri>()
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.id '{id}': {error}")))?;
    resolved.record(id, key, origin);
    Ok(())
}

/// The first typeless observation whose evolution the lookup did not resolve.
///
/// `resolving` is the half of the chunk that was looked up rather than upserted,
/// so it holds exactly the typeless observations and every deletion. A typeless
/// observation extends an evolution that must already exist, so a requested
/// identifier absent from the harvested keys is a `NotFound`. A deletion is not:
/// clause 5.6.16 removes a Temporal Evolution while clause 5.6.6 removes current
/// state, each on its own, so an entity whose evolution a client already deleted
/// holds no instance for the deletion to end and is passed over rather than
/// refused.
fn first_missing_typeless_observation<'a>(resolving: &[&'a TemporalCatalogUpsert], entity_keys: &HashMap<Uri, TemporalEntityKey>) -> Option<&'a Uri> {
    resolving
        .iter()
        .filter(|upsert| match upsert.purpose {
            TemporalWritePurpose::Observation => true,
            TemporalWritePurpose::Deletion => false,
        })
        .map(|upsert| &upsert.entity_id)
        .find(|id| !entity_keys.contains_key(*id))
}

/// The distinct entity types one chunk's creating upserts have to resolve.
fn distinct_types(creating: &[&TemporalCatalogUpsert]) -> Vec<EntityTypeName> {
    let mut names: Vec<EntityTypeName> = creating
        .iter()
        .flat_map(|upsert| upsert.types.iter().cloned()) // clone: dictionary resolution owns each distinct name.
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Column-major binds for one chunk's temporal-entity upsert.
struct EntityUpsertBinds<'a> {
    /// Identifier of each creating entity.
    ids: Vec<&'a str>,
    /// Write clock of each creating entity, one per entity so the array stays square.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Type dictionary keys of each creating entity, as a JSON array of integers.
    entity_type_name_keys_json: Vec<Value>,
}

/// Spread every creating entity into column-major binds, one row per entity.
fn entity_upsert_binds<'a>(
    creating: &[&'a TemporalCatalogUpsert],
    recorded_at: DateTime<Utc>,
    type_keys: &HashMap<EntityTypeName, EntityTypeNameKey>,
) -> Result<EntityUpsertBinds<'a>, TemporalStoreError> {
    let mut binds = EntityUpsertBinds {
        ids: Vec::with_capacity(creating.len()),
        recorded_ats: Vec::with_capacity(creating.len()),
        entity_type_name_keys_json: Vec::with_capacity(creating.len()),
    };
    for upsert in creating {
        let entity_type_name_keys = upsert
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
        binds.entity_type_name_keys_json.push(Value::from(entity_type_name_keys));
    }
    Ok(binds)
}

/// The three staging columns of the temporal-entity upsert, in bind order.
fn entity_staging_columns() -> [UnnestColumn; 3] {
    [
        UnnestColumn::new(EntityStaged::Id, PostgresType::TextArray),
        UnnestColumn::new(EntityStaged::RecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(EntityStaged::EntityTypeNameKeysJson, PostgresType::JsonbArray),
    ]
}

/// The temporal entities a chunk with declared types creates or refreshes.
///
/// `created_at` is set once and never moved: the conflict clause advances only
/// `modified_at` and the declared type keys, so the instant the Temporal
/// Evolution began survives every later write (clause 4.8). The write clock is
/// staged once per entity and read for both `created_at` and `modified_at`; the
/// type keys arrive as a JSONB array per entity and are expanded to `integer[]`
/// in the insert, the same interning `temporal_entities.entity_type_name_keys` carries. Every
/// minted key is harvested with `RETURNING`, so no read-back is needed, beside
/// `old.temporal_entity_key IS NULL`, which is true exactly for a row the insert
/// created rather than one its conflict action refreshed.
fn temporal_entity_upsert_batch_sql(target: &ContextTarget) -> Result<String, TemporalStoreError> {
    let columns = entity_staging_columns();
    let call = unnest_call([
        (&columns[0], external_placeholder(1)),
        (&columns[1], external_placeholder(2)),
        (&columns[2], external_placeholder(3)),
    ]);
    let entity_type_name_keys = array_subquery(
        &Query::select()
            .expr(cast(
                jsonb_array_elements_text(Expr::col((D, EntityStaged::EntityTypeNameKeysJson))).into(),
                PostgresType::Integer,
            ))
            .to_owned(),
    );

    let mut select = Query::select();
    select
        .expr(Expr::col((D, EntityStaged::Id)))
        .expr(Expr::col((D, EntityStaged::RecordedAt)))
        .expr(Expr::col((D, EntityStaged::RecordedAt)))
        .expr(entity_type_name_keys)
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table_of(target, TemporalEntities::Table)).columns([
        TemporalEntities::Id,
        TemporalEntities::CreatedAt,
        TemporalEntities::ModifiedAt,
        TemporalEntities::EntityTypeNameKeys,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::column(TemporalEntities::Id)
                .update_columns([TemporalEntities::ModifiedAt, TemporalEntities::EntityTypeNameKeys])
                .to_owned(),
        )
        .returning(Query::returning().exprs([
            Expr::col(TemporalEntities::TemporalEntityKey),
            Expr::col(TemporalEntities::Id),
            Expr::col((Old, TemporalEntities::TemporalEntityKey)).is_null(),
        ]));

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Read the narrow key of every typeless entity in one `id = ANY($1)` lookup.
///
/// `$1` is the requested identifiers as a `text[]`; a requested identifier the
/// read does not return has no temporal evolution to extend.
fn temporal_entity_keys_lookup_sql(target: &ContextTarget) -> String {
    Query::select()
        .columns([TemporalEntities::TemporalEntityKey, TemporalEntities::Id])
        .from(table_of(target, TemporalEntities::Table))
        .and_where(Expr::col(TemporalEntities::Id).eq(any(cast(external_placeholder(1), PostgresType::TextArray))))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::{
        temporal_entity_catalog_write::{
            EvolutionResolution,
            entity_upsert_binds,
            evolution_resolution,
            first_missing_typeless_observation,
            temporal_entity_keys_lookup_sql,
            temporal_entity_upsert_batch_sql,
        },
        temporal_stream::TemporalCatalogUpsert,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
    use andromeda_temporal::write_purpose::TemporalWritePurpose;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

    fn typed(entity_id: &str, types: &[&str]) -> TemporalCatalogUpsert {
        upsert(entity_id, types, TemporalWritePurpose::Observation)
    }

    fn typeless(entity_id: &str, purpose: TemporalWritePurpose) -> TemporalCatalogUpsert {
        upsert(entity_id, &[], purpose)
    }

    fn upsert(entity_id: &str, types: &[&str], purpose: TemporalWritePurpose) -> TemporalCatalogUpsert {
        TemporalCatalogUpsert {
            entity_id: entity_id.parse::<Uri>().expect("valid entity URI"),
            types: types.iter().map(|raw| raw.parse::<EntityTypeName>().expect("valid entity type")).collect(),
            purpose,
            streams: Vec::new(),
        }
    }

    fn resolved(entries: &[(&str, i64)]) -> HashMap<Uri, TemporalEntityKey> {
        entries
            .iter()
            .map(|(id, key)| {
                (
                    id.parse::<Uri>().expect("valid entity URI"),
                    TemporalEntityKey::try_from(*key).expect("a positive narrow key"),
                )
            })
            .collect()
    }

    #[test]
    fn the_batched_entity_upsert_returns_every_chunk_key() {
        let sql = temporal_entity_upsert_batch_sql(&ContextTarget::Live).expect("the batched entity upsert builds");
        assert!(sql.contains(r#"RETURNING "temporal_entity_key", "id""#), "{sql}");
        assert!(
            sql.contains("$1::text[]") && sql.contains("$2::timestamptz[]") && sql.contains("$3::jsonb[]"),
            "{sql}"
        );
        assert!(sql.contains("AS d(id, recorded_at, entity_type_name_keys_json)"), "{sql}");
    }

    /// The pre-image is NULL exactly for a row the insert created, which is how
    /// the chunk learns that an evolution owns no stream yet.
    #[test]
    fn the_batched_entity_upsert_reports_which_evolutions_it_created() {
        let sql = temporal_entity_upsert_batch_sql(&ContextTarget::Live).expect("the batched entity upsert builds");
        assert!(
            sql.ends_with(r#"RETURNING "temporal_entity_key", "id", "old"."temporal_entity_key" IS NULL"#),
            "{sql}"
        );
    }

    #[test]
    fn the_batched_entity_upsert_expands_the_type_keys_to_an_integer_array() {
        let sql = temporal_entity_upsert_batch_sql(&ContextTarget::Live).expect("the batched entity upsert builds");
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."entity_type_name_keys_json")::integer)"#),
            "{sql}"
        );
        assert!(!sql.contains("text[]) AS \"entity_type_name_keys\""), "{sql}");
    }

    #[test]
    fn a_repeated_write_never_moves_the_instant_the_evolution_began() {
        let sql = temporal_entity_upsert_batch_sql(&ContextTarget::Live).expect("the batched entity upsert builds");
        assert!(
            sql.contains(
                r#"ON CONFLICT ("id") DO UPDATE SET "modified_at" = "excluded"."modified_at", "entity_type_name_keys" = "excluded"."entity_type_name_keys""#
            ),
            "{sql}"
        );
        assert!(!sql.contains(r#""created_at" = "excluded""#), "{sql}");
    }

    #[test]
    fn the_typeless_lookup_matches_every_requested_identifier_in_one_array() {
        let sql = temporal_entity_keys_lookup_sql(&ContextTarget::Live);
        assert!(
            sql.starts_with(r#"SELECT "temporal_entity_key", "id" FROM "temporal_entities" WHERE "id" = ANY($1::text[])"#),
            "{sql}"
        );
    }

    /// A Snapshot branch qualifies the temporal catalogue table with its schema.
    #[test]
    fn a_snapshot_branch_qualifies_the_temporal_entities_table() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(4).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::published(key, id));
        assert!(temporal_entity_keys_lookup_sql(&target).contains(r#"FROM "snap_4"."temporal_entities""#));
    }

    /// Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated
    /// by clause 4.5.1", so an observation that says what the entity is may
    /// bring the evolution into being; clause 5.6.16 having removed it does not
    /// reserve the identifier against a later write.
    #[test]
    fn an_observation_that_states_its_entity_types_creates_the_evolution() {
        pretty_assertions::assert_eq!(evolution_resolution(&typed("urn:ngsi-ld:Vehicle:1", &[VEHICLE])), EvolutionResolution::Create);
    }

    /// Clause 4.5.1 makes at least one type part of what an Entity is, so an
    /// observation declaring none can only find an evolution already there.
    #[test]
    fn an_observation_that_states_no_entity_type_only_looks_the_evolution_up() {
        pretty_assertions::assert_eq!(
            evolution_resolution(&typeless("urn:ngsi-ld:Vehicle:2", TemporalWritePurpose::Observation)),
            EvolutionResolution::Lookup
        );
    }

    /// Clauses 4.5.7 and 4.5.8 record a deletion as one more instance of a
    /// sequence, so a deletion that meets no sequence has nothing to end — and
    /// creating an evolution out of an ending would assert instances that the
    /// client's clause 5.6.16 deletion removed. A deletion never creates, even
    /// when the entity it ended attributes of states its types.
    #[test]
    fn a_deletion_never_creates_the_evolution_even_when_it_states_entity_types() {
        pretty_assertions::assert_eq!(
            evolution_resolution(&upsert("urn:ngsi-ld:Vehicle:3", &[VEHICLE], TemporalWritePurpose::Deletion)),
            EvolutionResolution::Lookup
        );
    }

    #[test]
    fn the_upsert_binds_only_the_typed_entities_and_squares_the_arrays() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let type_keys = HashMap::from([(VEHICLE.parse::<EntityTypeName>().expect("valid entity type"), EntityTypeNameKey::from(4_i32))]);
        let typed = [typed("urn:ngsi-ld:Vehicle:1", &[VEHICLE]), typed("urn:ngsi-ld:Vehicle:2", &[VEHICLE])];
        let refs: Vec<&TemporalCatalogUpsert> = typed.iter().collect();
        let binds = entity_upsert_binds(&refs, recorded_at, &type_keys).expect("known types");
        pretty_assertions::assert_eq!(binds.ids, vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
        pretty_assertions::assert_eq!(binds.recorded_ats, vec![recorded_at, recorded_at]);
        pretty_assertions::assert_eq!(binds.entity_type_name_keys_json.len(), 2);
        pretty_assertions::assert_eq!(binds.entity_type_name_keys_json[0], serde_json::json!([4]));
    }

    /// Clause 4.5.1 makes at least one type part of what an Entity is, so an
    /// observation that declares none can only extend an evolution that is
    /// already there; one that is not is reported missing rather than invented.
    #[test]
    fn a_typeless_observation_for_a_missing_evolution_is_reported_missing() {
        let upserts = [typeless("urn:ngsi-ld:Vehicle:2", TemporalWritePurpose::Observation)];
        let resolving: Vec<&TemporalCatalogUpsert> = upserts.iter().collect();
        let missing = first_missing_typeless_observation(&resolving, &resolved(&[("urn:ngsi-ld:Vehicle:1", 1)])).expect("the typeless evolution is missing");
        pretty_assertions::assert_eq!(missing.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    #[test]
    fn a_typeless_observation_whose_evolution_exists_is_not_reported_missing() {
        let upserts = [typeless("urn:ngsi-ld:Vehicle:2", TemporalWritePurpose::Observation)];
        let resolving: Vec<&TemporalCatalogUpsert> = upserts.iter().collect();
        assert!(first_missing_typeless_observation(&resolving, &resolved(&[("urn:ngsi-ld:Vehicle:2", 9)])).is_none());
    }

    /// Clause 5.6.16 removes a Temporal Evolution on its own and clause 5.6.6
    /// removes current state on its own, so an Entity can outlive its history.
    /// Clauses 4.5.7 and 4.5.8 record a deletion as one more instance of a
    /// sequence, and a deletion that meets no sequence has no instance to end:
    /// it is passed over rather than reported missing.
    #[test]
    fn a_typeless_deletion_for_a_missing_evolution_is_not_reported_missing() {
        let upserts = [typeless("urn:ngsi-ld:Vehicle:2", TemporalWritePurpose::Deletion)];
        let resolving: Vec<&TemporalCatalogUpsert> = upserts.iter().collect();
        assert!(first_missing_typeless_observation(&resolving, &HashMap::new()).is_none());
    }

    /// A deletion resolves by lookup whatever it declares, so a typed one whose
    /// evolution clause 5.6.16 already removed reaches the same pass-over.
    #[test]
    fn a_typed_deletion_for_a_missing_evolution_is_not_reported_missing() {
        let upserts = [upsert("urn:ngsi-ld:Vehicle:2", &[VEHICLE], TemporalWritePurpose::Deletion)];
        let resolving: Vec<&TemporalCatalogUpsert> = upserts.iter().collect();
        assert!(first_missing_typeless_observation(&resolving, &HashMap::new()).is_none());
    }

    /// A chunk mixing both purposes reports only the observation: the deletion
    /// beside it is not a reason to fail, and is not a reason to let a missing
    /// observation through either.
    #[test]
    fn a_chunk_reports_its_missing_observation_and_passes_over_its_missing_deletion() {
        let upserts = [
            typeless("urn:ngsi-ld:Vehicle:1", TemporalWritePurpose::Deletion),
            typeless("urn:ngsi-ld:Vehicle:2", TemporalWritePurpose::Observation),
        ];
        let resolving: Vec<&TemporalCatalogUpsert> = upserts.iter().collect();
        let missing = first_missing_typeless_observation(&resolving, &HashMap::new()).expect("the observation is missing");
        pretty_assertions::assert_eq!(missing.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    #[test]
    fn the_entity_catalogue_names_no_current_state_relation() {
        for statement in [
            temporal_entity_upsert_batch_sql(&ContextTarget::Live).expect("the batched entity upsert builds"),
            temporal_entity_keys_lookup_sql(&ContextTarget::Live),
        ] {
            assert!(!statement.contains(r#""entities""#), "{statement}");
        }
    }
}
