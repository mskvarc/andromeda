//! The batch insert: one statement that lands each staged entity in all three
//! current-state relations.

use crate::{
    entity::{
        mapper::stored_written_entity,
        params::PreparedEntityColumns,
        repo::EntityRepository,
        types_present::{entity_write_failure, types_present},
        write::{
            staging::{Docs, ExpectationStaged, Ins, Proj, Srch, Staged, from_inserted, insert_staging_columns, projected, projection_cte},
            type_membership::{Mbrs, created_memberships},
        },
        written_entity::WrittenEntity,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::unnest::apply_column_alias;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{CommonTableExpression, Expr, OnConflict, PostgresQueryBuilder, Query, WithClause};
use sqlx::{Postgres, Transaction};

/// The statement name the entity set-insert is observed under.
const ENTITY_INSERT: StatementName = StatementName("entity-insert");

/// Build the batch insert.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST(…) AS d(…)),
///      ins AS (INSERT INTO entities (id, expires_at, min_attribute_expires_at)
///              SELECT … FROM proj ON CONFLICT (id) DO NOTHING RETURNING entity_key, id),
///      docs AS (INSERT INTO entity_documents (entity_key, payload)
///               SELECT ins.entity_key, proj.payload FROM ins JOIN proj ON proj.id = ins.id),
///      srch AS (INSERT INTO entity_search (…)
///               SELECT ins.entity_key, … FROM ins JOIN proj ON proj.id = ins.id),
///      mbrs AS (INSERT INTO entity_type_members (entity_key, entity_type_name_key, id)
///               SELECT ins.entity_key, carried.entity_type_name_key, ins.id FROM ins JOIN proj ON proj.id = ins.id
///               INNER JOIN LATERAL (SELECT UNNEST(proj.entity_type_name_keys) …) AS carried ON TRUE)
/// SELECT entity_key, id FROM ins
/// ```
///
/// The conflict clause on `entities` is how a duplicate create is detected:
/// the identifier simply does not come back, and the caller answers 409 off
/// that (NGSI-LD 6.4.3.1). Suppressing the conflict rather than raising a
/// duplicate-key error is what lets one statement serve a batch that is
/// permitted to succeed partially (clause 5.6.7) and the idempotent
/// placeholder the Temporal API needs, without three different statements. The
/// child inserts read the keys the identity insert returned, so an entity that
/// already existed lands in no relation at all.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if a child projection ever stops
/// matching its column list, which the fixed shapes here make unreachable.
pub(super) fn entity_insert_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = insert_staging_columns();

    let mut identity = Query::insert();
    identity
        .into_table(table_of(target, Entities::Table))
        .columns([Entities::Id, Entities::ExpiresAt, Entities::MinAttributeExpiresAt])
        .select_from(
            Query::select()
                .expr(projected(Entities::Id))
                .expr(projected(Entities::ExpiresAt))
                .expr(projected(Entities::MinAttributeExpiresAt))
                .from(Proj)
                .take(),
        )
        .map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    identity
        .on_conflict(OnConflict::column(Entities::Id).do_nothing().to_owned())
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]));

    let mut documents = Query::insert();
    documents
        .into_table(table_of(target, EntityDocuments::Table))
        .columns([EntityDocuments::EntityKey, EntityDocuments::Payload])
        .select_from(
            from_inserted()
                .expr(Expr::col((Ins, Entities::EntityKey)))
                .expr(projected(EntityDocuments::Payload))
                .take(),
        )
        .map_err(|error| ContextStoreError::Internal(error.to_string()))?;

    let mut search = Query::insert();
    search
        .into_table(table_of(target, EntitySearch::Table))
        .columns([
            EntitySearch::EntityKey,
            EntitySearch::LocationExpiresAt,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeNameKeys,
            EntitySearch::ScopePaths,
            EntitySearch::Location,
            EntitySearch::ObservationSpace,
            EntitySearch::OperationSpace,
        ])
        .select_from(
            from_inserted()
                .expr(Expr::col((Ins, Entities::EntityKey)))
                .expr(projected(EntitySearch::LocationExpiresAt))
                .expr(projected(EntitySearch::EntityTypeNameKeys))
                .expr(projected(EntitySearch::AttributeNameKeys))
                .expr(projected(EntitySearch::ScopePaths))
                .expr(projected(EntitySearch::Location))
                .expr(projected(EntitySearch::ObservationSpace))
                .expr(projected(EntitySearch::OperationSpace))
                .take(),
        )
        .map_err(|error| ContextStoreError::Internal(error.to_string()))?;

    let statement = WithClause::new()
        .cte(projection_cte(&columns, ExpectationStaged::No))
        .cte(CommonTableExpression::new().table_name(Ins).query(identity).to_owned())
        .cte(CommonTableExpression::new().table_name(Docs).query(documents).to_owned())
        .cte(CommonTableExpression::new().table_name(Srch).query(search).to_owned())
        .cte(CommonTableExpression::new().table_name(Mbrs).query(created_memberships(target)?).to_owned())
        .to_owned()
        .query(Query::select().columns([Entities::EntityKey, Entities::Id]).from(Ins).take());

    let sql = statement.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &Staged, &columns))
}

impl EntityRepository {
    /// Insert every entity in `entities` that does not already exist, returning
    /// the rows actually inserted.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when an entity carries no type, when the
    /// insert statement cannot be built, when the statement fails, or when a
    /// returned surrogate key or identifier is not a valid written entity.
    pub async fn create_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        entities: &PreparedEntityColumns,
    ) -> Result<Vec<WrittenEntity>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        types_present(entities)?;

        let ids = entities.ids.iter().map(Uri::as_str).collect::<Vec<_>>();
        let sql = entity_insert_sql(target)?;
        let rows = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&ids)
            .bind(&entities.entity_type_name_keys_json)
            .bind(&entities.attribute_name_keys_json)
            .bind(&entities.scope_paths_json)
            .bind(&entities.payload_json)
            .bind(&entities.locations)
            .bind(&entities.obs_spaces)
            .bind(&entities.op_spaces)
            .bind(&entities.location_expires_ats)
            .bind(&entities.expires_ats)
            .bind(&entities.min_attribute_expires_ats)
            .fetch_all(&mut **tx);
        let rows: Vec<(i64, String)> = observe_statement(ENTITY_INSERT, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(key, id)| stored_written_entity(*key, id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::write::{insert::entity_insert_sql, placeholders};
    use andromeda_context_space::context_target::ContextTarget;
    use std::collections::BTreeSet;

    /// The rendered insert, unwrapped for assertion.
    fn insert() -> String {
        entity_insert_sql(&ContextTarget::Live).expect("the fixed eleven-wide insert builds")
    }

    #[test]
    fn the_insert_binds_contiguous_placeholders_and_names_its_staging_columns() {
        let sql = insert();
        pretty_assertions::assert_eq!(placeholders(&sql), (1..=11).collect::<BTreeSet<u32>>());
        assert!(
            sql.contains(
                "AS d(id, entity_type_name_keys_json, attribute_name_keys_json, scope_paths_json, payload, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at)"
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_insert_projects_once_and_lands_in_three_relations() {
        let sql = insert();
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
        assert!(
            sql.contains(r#""ins" AS (INSERT INTO "entities" ("id", "expires_at", "min_attribute_expires_at")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""docs" AS (INSERT INTO "entity_documents" ("entity_key", "payload")"#), "{sql}");
        assert!(sql.contains(r#""srch" AS (INSERT INTO "entity_search" ("entity_key", "location_expires_at", "entity_type_name_keys", "attribute_name_keys", "scope_paths", "location", "observation_space", "operation_space")"#), "{sql}");
        assert!(sql.ends_with(r#"SELECT "entity_key", "id" FROM "ins""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches("ST_MakeValid").count(),
            3,
            "each geometry is validated once, in the projection: {sql}"
        );
    }

    #[test]
    fn the_insert_expands_the_key_and_scope_arrays_and_casts_each_to_its_element_type() {
        let sql = insert();
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."entity_type_name_keys_json")::integer) AS "entity_type_name_keys""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."attribute_name_keys_json")::integer) AS "attribute_name_keys""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."scope_paths_json")::ltree) AS "scope_paths""#),
            "{sql}"
        );
    }

    #[test]
    fn the_insert_validates_every_geometry_from_its_embedded_srid() {
        let sql = insert();
        for column in ["loc", "obs", "ops"] {
            assert!(sql.contains(&format!(r#"ST_MakeValid(ST_GeomFromEWKB("d"."{column}"))"#)), "{sql}");
        }
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }

    #[test]
    fn the_insert_keeps_its_conflict_clause_and_writes_no_version_token() {
        let sql = insert();
        assert!(sql.contains(r#"ON CONFLICT ("id") DO NOTHING RETURNING "entity_key", "id""#), "{sql}");
        assert!(!sql.contains("modified_at"), "the token defaults in the database: {sql}");
        assert!(!sql.contains("location_geography"), "the geography is generated: {sql}");
    }

    #[test]
    fn the_child_inserts_read_the_minted_key_beside_the_projected_row() {
        let sql = insert();
        pretty_assertions::assert_eq!(sql.matches(r#"FROM "ins" JOIN "proj" ON "proj"."id" = "ins"."id""#).count(), 3, "{sql}");
    }

    /// Clause 4.17 selects on membership, so a create lands one membership row
    /// per type beside the search row that carries the same keys.
    #[test]
    fn a_create_lands_its_type_memberships_in_the_same_statement() {
        let sql = insert();
        assert!(
            sql.contains(r#""mbrs" AS (INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id")"#),
            "{sql}"
        );
    }

    /// A Snapshot branch lands its rows in the Snapshot's own schema, so every
    /// target relation of the insert is schema-qualified.
    #[test]
    fn a_snapshot_branch_lands_in_the_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, id));
        let sql = entity_insert_sql(&target).expect("insert builds");
        assert!(sql.contains(r#"INSERT INTO "snap_5_stg"."entities""#), "{sql}");
        assert!(sql.contains(r#"INSERT INTO "snap_5_stg"."entity_documents""#), "{sql}");
        assert!(sql.contains(r#"INSERT INTO "snap_5_stg"."entity_search""#), "{sql}");
        assert!(sql.contains(r#"INSERT INTO "snap_5_stg"."entity_type_members""#), "{sql}");
    }
}
