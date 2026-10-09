//! The batch insert: one statement that lands each staged entity in every
//! current-state relation, its attribute instances included.

use crate::{
    branch_statement::BranchStatement,
    current_attribute::{
        staged_rows::StagedRows,
        staging::{apply_instances_alias, instances_cte},
        write::inserted_rows,
    },
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_uri,
        params::{PreparedEntityColumns, literal_texts},
        repo::EntityRepository,
        staged_geometry::written_column,
        types_present::{entity_write_failure, types_present},
        write::{
            search_image::SearchImage,
            staging::{Attrs, Ins, Proj, ProjectedWrite, Srch, apply_staged_alias, from_inserted, insert_staging_columns, projected, projection_cte},
            type_membership::{Mbrs, created_memberships},
        },
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{CommonTableExpression, Expr, IntoIden, OnConflict, PostgresQueryBuilder, Query, WithClause};
use sqlx::{Postgres, Transaction};

/// The statement name the entity set-insert is observed under.
const ENTITY_INSERT: StatementName = StatementName("entity-insert");

/// The batch insert's text, built once for the live branch.
static ENTITY_INSERT_STATEMENT: BranchStatement = BranchStatement::new(entity_insert_sql);

/// Build the batch insert.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST($1::text[], …, $13::timestamptz[]) AS d(…)),
///      ins AS (INSERT INTO entities (id, expires_at, min_attribute_expires_at)
///              SELECT … FROM proj ON CONFLICT (id) DO NOTHING RETURNING entity_key, id),
///      srch(entity_key, new_types, new_instance_names, new_instance_kinds) AS
///              (INSERT INTO entity_search (…)
///               SELECT ins.entity_key, … FROM ins JOIN proj ON proj.id = ins.id
///               RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds),
///      mbrs AS (INSERT INTO entity_type_members (entity_key, entity_type_name_key, id)
///               SELECT ins.entity_key, carried.entity_type_name_key, ins.id FROM ins JOIN proj ON proj.id = ins.id
///               INNER JOIN LATERAL (SELECT UNNEST(proj.entity_type_name_keys) …) AS carried ON TRUE),
///      instances AS (SELECT r.id, r.attribute_name_key, … FROM UNNEST($14::text[], …) AS r(…)),
///      attrs AS (INSERT INTO current_attributes (entity_key, …)
///                SELECT ins.entity_key, instances.… FROM ins JOIN instances ON instances.id = ins.id),
///      census_types AS (INSERT INTO entity_type_census … FROM srch …),
///      census_type_attributes AS (INSERT INTO entity_type_attribute_census … FROM srch …),
///      census_attributes AS (INSERT INTO attribute_census … FROM srch …)
/// SELECT id FROM ins
/// ```
///
/// The conflict clause on `entities` is how a duplicate create is detected:
/// the identifier simply does not come back, and the caller answers 409 off
/// that (NGSI-LD 6.4.3.1). Suppressing the conflict rather than raising a
/// duplicate-key error is what lets one statement serve a batch that is
/// permitted to succeed partially (clause 5.6.7) and the idempotent
/// placeholder the Temporal API needs, without three different statements. The
/// child inserts read the keys the identity insert returned, so an entity that
/// already existed lands in no relation at all -- and, because the census reads
/// only the search rows the statement returned, is counted by no census ledger
/// either.
///
/// Every attribute instance is one row, whatever the nesting of its
/// sub-attributes, so one insert lands every instance of every entity, paired
/// with its entity by identifier.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if a child projection ever stops
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
        )?;
    identity
        .on_conflict(OnConflict::column(Entities::Id).do_nothing().to_owned())
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]));

    let mut search = Query::insert();
    search
        .into_table(table_of(target, EntitySearch::Table))
        .columns([
            EntitySearch::EntityKey,
            EntitySearch::LocationExpiresAt,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
            EntitySearch::ScopePaths,
            EntitySearch::VerbatimScopePaths,
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
                .expr(projected(EntitySearch::AttributeInstanceNameKeys))
                .expr(projected(EntitySearch::AttributeInstanceKinds))
                .expr(projected(EntitySearch::ScopePaths))
                .expr(projected(EntitySearch::VerbatimScopePaths))
                .expr(projected(EntitySearch::Location))
                .expr(projected(EntitySearch::ObservationSpace))
                .expr(projected(EntitySearch::OperationSpace))
                .take(),
        )?;
    search.returning(Query::returning().columns([
        EntitySearch::EntityKey,
        EntitySearch::EntityTypeNameKeys,
        EntitySearch::AttributeInstanceNameKeys,
        EntitySearch::AttributeInstanceKinds,
    ]));

    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, ProjectedWrite::Insert))
        .cte(CommonTableExpression::new().table_name(Ins).query(identity).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(Srch)
                .columns([
                    EntitySearch::EntityKey.into_iden(),
                    SearchImage::NewTypes.into_iden(),
                    SearchImage::NewInstanceNames.into_iden(),
                    SearchImage::NewInstanceKinds.into_iden(),
                ])
                .query(search)
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Mbrs).query(created_memberships(target)?).to_owned())
        .cte(instances_cte(columns.len() + 1))
        .cte(CommonTableExpression::new().table_name(Attrs).query(inserted_rows(target, Ins)?).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Added)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(Query::select().column(Entities::Id).from(Ins).take());

    let sql = statement.to_string(PostgresQueryBuilder);
    Ok(apply_instances_alias(apply_staged_alias(sql, &columns)))
}

impl EntityRepository {
    /// Insert every entity in `entities` that does not already exist, with the
    /// attribute instance rows `rows` stages for it, returning the entities
    /// actually inserted.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when an entity carries no type, when the
    /// insert statement cannot be built, when the statement fails, or when a
    /// returned identifier is not a valid URI.
    pub async fn create_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        entities: &PreparedEntityColumns,
        rows: StagedRows<'_>,
    ) -> Result<Vec<Identifier>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        types_present(entities)?;

        let ids = entities.ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
        let locations = written_column(&entities.locations)?;
        let observation_spaces = written_column(&entities.obs_spaces)?;
        let operation_spaces = written_column(&entities.op_spaces)?;
        let sql = ENTITY_INSERT_STATEMENT.text(target)?;
        let query = sqlx::query_as(sqlx::AssertSqlSafe(&*sql))
            .bind(&ids)
            .bind(literal_texts(&entities.entity_type_name_keys))
            .bind(literal_texts(&entities.attribute_name_keys))
            .bind(literal_texts(&entities.attribute_instance_name_keys))
            .bind(literal_texts(&entities.attribute_instance_kinds))
            .bind(entities.scope_paths_bind())
            .bind(entities.verbatim_scope_paths_bind())
            .bind(&locations)
            .bind(&observation_spaces)
            .bind(&operation_spaces)
            .bind(&entities.location_expires_ats)
            .bind(&entities.expires_ats)
            .bind(&entities.min_attribute_expires_ats);
        let rows = rows.bind_to(query).fetch_all(&mut **tx);
        let rows: Vec<(String,)> = observe_statement(ENTITY_INSERT, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(id,)| stored_uri(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute::staging::ROW_COLUMNS,
        entity::write::{insert::entity_insert_sql, placeholders, staging::insert_staging_columns},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use std::collections::BTreeSet;

    /// The rendered insert, unwrapped for assertion.
    fn insert() -> String {
        entity_insert_sql(&ContextTarget::Live).expect("the fixed insert builds")
    }

    #[test]
    fn the_insert_binds_contiguous_placeholders_and_names_its_staging_columns() {
        let sql = insert();
        let binds = insert_staging_columns().len() + ROW_COLUMNS.len() + 1;
        pretty_assertions::assert_eq!(
            placeholders(&sql),
            (1..=u32::try_from(binds).expect("a small count")).collect::<BTreeSet<u32>>()
        );
        assert!(
            sql.contains(
                " AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, verbatim_scope_paths_json, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at)"
            ),
            "{sql}"
        );
        assert!(sql.contains(" AS r(id, attribute_name_key, dataset_id, ordinal, "), "{sql}");
    }

    /// Every attribute instance is one row whatever its nesting, so a create
    /// lands every depth with one insert from the row staging, and no relation
    /// beside the row holds an object or a level.
    #[test]
    fn a_create_writes_every_depth_in_one_insert() {
        let sql = insert();
        pretty_assertions::assert_eq!(sql.matches(r#"INSERT INTO "current_attributes""#).count(), 1, "{sql}");
        assert!(
            sql.contains(r#""attrs" AS (INSERT INTO "current_attributes" ("entity_key", "attribute_name_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#""instances" AS (SELECT "r"."id" AS "id""#), "{sql}");
        assert!(sql.contains(r"FROM UNNEST($14::text[]"), "{sql}");
        assert!(sql.contains(r"$40::sub_attribute_tree[]) AS r("), "{sql}");
        assert!(sql.contains(r#""r"."sub_attributes" AS "sub_attributes""#), "{sql}");
        assert!(!sql.contains("WITH RECURSIVE"), "{sql}");
    }

    /// A row finds its entity by identifier: a positional pairing has no index,
    /// and under a generic plan's row estimate PostgreSQL rescans the staging
    /// once per entity.
    #[test]
    fn a_create_pairs_rows_by_entity_id() {
        let sql = insert();
        assert!(sql.contains(r#"FROM "ins" JOIN "instances" ON "instances"."id" = "ins"."id""#), "{sql}");
        assert!(!sql.to_lowercase().contains("ordinality"), "{sql}");
    }

    #[test]
    fn the_insert_projects_once_and_lands_in_every_relation() {
        let sql = insert();
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
        assert!(
            sql.contains(r#""ins" AS (INSERT INTO "entities" ("id", "expires_at", "min_attribute_expires_at")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""srch" ("entity_key", "new_types", "new_instance_names", "new_instance_kinds") AS (INSERT INTO "entity_search" ("entity_key", "location_expires_at", "entity_type_name_keys", "attribute_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds", "scope_paths", "verbatim_scope_paths", "location", "observation_space", "operation_space")"#), "{sql}");
        assert!(
            sql.contains(r#"RETURNING "entity_key", "entity_type_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds")"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"SELECT "id" FROM "ins""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches("ST_MakeValid").count(),
            4,
            "each hoisted geometry is validated once, in the projection, and each instance geometry once, in the row staging: {sql}"
        );
    }

    /// The census counts the search rows the insert returned, so an entity the
    /// conflict clause skipped is counted by no ledger.
    #[test]
    fn the_insert_counts_what_it_inserted_into_the_census() {
        let sql = insert();
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
        pretty_assertions::assert_eq!(sql.matches(r#"FROM "srch""#).count(), 1, "the census reads the returned rows once: {sql}");
    }

    /// The integer arrays are cast from their array literals, which the array
    /// type's input function reads at once; the scopes expand from their JSON
    /// arrays, once as label paths and once as written.
    #[test]
    fn the_insert_casts_the_key_literals_and_expands_the_scope_arrays() {
        let sql = insert();
        for (literal, column, element) in [
            ("entity_type_name_keys_literal", "entity_type_name_keys", "integer"),
            ("attribute_name_keys_literal", "attribute_name_keys", "integer"),
            ("attribute_instance_name_keys_literal", "attribute_instance_name_keys", "integer"),
            ("attribute_instance_kinds_literal", "attribute_instance_kinds", "smallint"),
        ] {
            assert!(sql.contains(&format!(r#""d"."{literal}"::{element}[] AS "{column}""#)), "{column}: {sql}");
        }
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."scope_paths_json")::ltree) AS "scope_paths""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ARRAY(SELECT jsonb_array_elements_text("d"."verbatim_scope_paths_json")::text) AS "verbatim_scope_paths""#),
            "{sql}"
        );
        assert!(sql.contains(r#""r"."object_type_keys"::integer[] AS "object_type_keys""#), "{sql}");
    }

    #[test]
    fn the_insert_validates_every_geometry_from_its_embedded_srid() {
        let sql = insert();
        for column in [r#""d"."loc""#, r#""d"."obs""#, r#""d"."ops""#, r#""r"."value_geometry""#] {
            assert!(sql.contains(&format!("ST_MakeValid(ST_GeomFromEWKB({column}))")), "{column}: {sql}");
        }
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }

    /// The entity's own `createdAt` and `modifiedAt` default to the
    /// transaction's instant; only the instance rows carry stamped times.
    #[test]
    fn the_insert_keeps_its_conflict_clause_and_writes_no_entity_version_token() {
        let sql = insert();
        assert!(sql.contains(r#"ON CONFLICT ("id") DO NOTHING RETURNING "entity_key", "id""#), "{sql}");
        let identity = sql.split(r#""srch""#).next().unwrap_or_default();
        assert!(!identity.contains("modified_at"), "the token defaults in the database: {identity}");
        assert!(!sql.contains("location_geography"), "the geography is generated: {sql}");
    }

    /// Every child insert finds its row by the identifier the identity insert
    /// returned: the search and membership rows in the projection, the
    /// attribute instance rows in the row staging.
    #[test]
    fn the_child_inserts_read_the_minted_key_beside_the_staged_row() {
        let sql = insert();
        pretty_assertions::assert_eq!(sql.matches(r#"FROM "ins" JOIN "proj" ON "proj"."id" = "ins"."id""#).count(), 2, "{sql}");
        assert!(sql.contains(r#"SELECT "ins"."entity_key", "instances"."attribute_name_key""#), "{sql}");
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
        for relation in [
            "entities",
            "entity_search",
            "entity_type_members",
            "current_attributes",
            "entity_type_attribute_census",
        ] {
            assert!(sql.contains(&format!(r#"INSERT INTO "snap_5_stg"."{relation}""#)), "{relation}: {sql}");
        }
    }
}
