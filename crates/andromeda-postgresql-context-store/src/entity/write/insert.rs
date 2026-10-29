//! The batch insert: one statement that lands each staged entity in every
//! current-state relation, its attribute instances included.

use crate::{
    current_attribute::{
        staged_rows::StagedRows,
        staging::{instances_cte, with_instances_alias},
        write::inserted_rows,
    },
    entity::{
        census::delta::{CensusChange, CensusStatements},
        created_entities::CreatedEntities,
        mapper::stored_uri,
        params::PreparedEntityColumns,
        repo::EntityRepository,
        staged_geometry::written_column,
        types_present::{entity_write_failure, types_present},
        write::{
            search_image::SearchImage,
            staging::{
                Attrs,
                Established,
                Ins,
                Proj,
                ProjectedWrite,
                Srch,
                StagedColumn,
                entity_arrays,
                from_inserted,
                insert_staging_columns,
                projected,
                projection_cte,
                with_staged_alias,
            },
            type_membership::{Mbrs, created_memberships},
            version_guard::next_write_count,
        },
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    external_arguments::{ExternalArguments, ExternallyBound},
    returning_image::{New, Old},
    unnest::UnnestAliased,
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{
    CaseStatement,
    CommonTableExpression,
    Expr,
    ExprTrait,
    InsertStatement,
    IntoIden,
    Keyword,
    OnConflict,
    Query,
    SimpleExpr,
    WithClause,
    WithQuery,
};
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;

/// The statement name the entity set-insert is observed under.
const ENTITY_INSERT: StatementName = StatementName("entity-insert");

/// Build the batch insert of `entities` and the attribute instance rows
/// `rows` stages for them, each entity created and modified at `accepted_at`,
/// establishing the Temporal Evolution of those in `establishing`.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST($1::text[], …, $12::boolean[]) AS d(…)),
///      ins(entity_key, id, established) AS (INSERT INTO entities (id, created_at, modified_at, expires_at, min_attribute_expires_at,
///                                                    evolution_created_at, evolution_modified_at, evolution_type_name_keys)
///              SELECT proj.id, $accepted_at, $accepted_at, …, CASE WHEN proj.establishes_evolution THEN $accepted_at END, … FROM proj
///              ON CONFLICT (id) DO UPDATE SET created_at = excluded.created_at, …,
///              write_count = entities.write_count + 1 WHERE entities.created_at IS NULL
///              RETURNING new.entity_key, new.id, old.entity_key IS NULL AND new.evolution_created_at IS NOT NULL),
///      srch(entity_key, new_types, new_instance_names, new_instance_kinds) AS
///              (INSERT INTO entity_search (…)
///               SELECT ins.entity_key, … FROM ins JOIN proj ON proj.id = ins.id
///               RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds),
///      mbrs AS (INSERT INTO entity_type_members (entity_key, entity_type_name_key, id, is_current_member, is_evolution_member)
///               SELECT ins.entity_key, carried.entity_type_name_key, ins.id, TRUE, ins.established FROM ins JOIN proj ON proj.id = ins.id
///               INNER JOIN LATERAL (SELECT UNNEST(proj.entity_type_name_keys) …) AS carried ON TRUE
///               ON CONFLICT (entity_type_name_key, id) DO UPDATE SET is_current_member = TRUE),
///      instances AS (SELECT r.id, r.attribute_name_key, … FROM UNNEST($12::text[], …) AS r(…)),
///      attrs AS (INSERT INTO current_attributes (entity_key, …)
///                SELECT ins.entity_key, instances.… FROM ins JOIN instances ON instances.id = ins.id),
///      census_types AS (INSERT INTO entity_type_census … FROM srch …),
///      census_type_attributes AS (INSERT INTO entity_type_attribute_census … FROM srch …),
///      census_attributes AS (INSERT INTO attribute_census … FROM srch …)
/// SELECT id, established FROM ins
/// ```
///
/// The conflict clause on `entities` is how a duplicate create is detected:
/// an Entity with a current state is left alone, its identifier does not come
/// back, and the caller answers 409 off that (NGSI-LD 6.4.3.1). Suppressing the
/// conflict rather than raising a duplicate-key error is what lets one
/// statement serve a batch that is permitted to succeed partially (clause
/// 5.6.7). The child inserts read the keys the identity insert returned, so an
/// entity that already existed lands in no relation at all -- and, because the
/// census reads only the search rows the statement returned, is counted by no
/// census ledger either.
///
/// An identity row without a current state is the Temporal Evolution of an
/// Entity a core deletion removed (clause 5.6.6) or the Temporal API created.
/// Clause 5.6.1 creates the Entity anew under the same identifier, so the
/// conflict action gives that row its current state and returns it: its
/// Temporal Evolution continues under the same key, and its type memberships
/// gain the current flag. It counts the creation as a write rather than
/// starting the count over, so a version read before the deletion never
/// matches the Entity created anew; a new row starts at one.
///
/// Every attribute instance is one row, whatever the nesting of its
/// sub-attributes, so one insert lands every instance of every entity, paired
/// with its entity by identifier.
///
/// An entity in `establishing` is one whose commit records its history in
/// the same transaction, and that history creates its Temporal Evolution
/// (clause 4.5.6) at `accepted_at` under the types the create states. A new
/// identity row is minted with that evolution and with its memberships on
/// both sides, which the history application then leaves as they are, so
/// each row is written once. The conflict action does not touch the
/// evolution columns, so an identity row that existed keeps the evolution
/// it held, and `established` says which rows were minted with one.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if a child projection ever stops
/// matching its column list, which the fixed shapes here make unreachable.
pub(super) fn entity_insert_statement<'entities>(
    target: &ContextTarget,
    entities: &'entities PreparedEntityColumns,
    rows: StagedRows<'_>,
    accepted_at: DateTime<Utc>,
    establishing: &HashSet<Identifier>,
) -> Result<ExternallyBound<'entities, UnnestAliased<WithQuery>>, ContextStoreError> {
    let columns = insert_staging_columns();
    let mut external = ExternalArguments::default();
    let mut staged_entities = entity_arrays(entities, &written_column(&entities.locations)?, &mut external);
    staged_entities.push(Expr::val(entities.ids.iter().map(|id| establishing.contains(id)).collect::<Vec<bool>>()));
    let instance_arrays = rows.into_arrays(&mut external);

    let identity = identity_insert(target, accepted_at)?;

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
            EntitySearch::LocationReach,
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
                .expr(projected(EntitySearch::LocationReach))
                .take(),
        )?;
    search.returning(Query::returning().columns([
        EntitySearch::EntityKey,
        EntitySearch::EntityTypeNameKeys,
        EntitySearch::AttributeInstanceNameKeys,
        EntitySearch::AttributeInstanceKinds,
    ]));

    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, staged_entities, ProjectedWrite::Insert))
        .cte(
            CommonTableExpression::new()
                .table_name(Ins)
                .columns([Entities::EntityKey.into_iden(), Entities::Id.into_iden(), Established.into_iden()])
                .query(identity)
                .to_owned(),
        )
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
        .cte(instances_cte(instance_arrays))
        .cte(CommonTableExpression::new().table_name(Attrs).query(inserted_rows(target, Ins)?).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Added)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(Query::select().column(Entities::Id).column(Established).from(Ins).take());

    let statement = with_instances_alias(with_staged_alias(UnnestAliased::new(statement), &columns));
    Ok(ExternallyBound::new(statement, external))
}

/// The identity insert of a create, `ins`: each staged entity created and
/// modified at `accepted_at`, minted with its Temporal Evolution where it is
/// staged as establishing one, returning its key, its identifier and whether
/// the insert minted the row with that evolution.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the projection ever stops
/// matching the column list, which the fixed shape makes unreachable.
fn identity_insert(target: &ContextTarget, accepted_at: DateTime<Utc>) -> Result<InsertStatement, ContextStoreError> {
    let mut identity = Query::insert();
    let current_state = [Entities::CreatedAt, Entities::ModifiedAt, Entities::ExpiresAt, Entities::MinAttributeExpiresAt];
    let evolution = [Entities::EvolutionCreatedAt, Entities::EvolutionModifiedAt, Entities::EvolutionTypeNameKeys];
    let established = |value: SimpleExpr| -> SimpleExpr {
        CaseStatement::new()
            .case(projected(StagedColumn::EstablishesEvolution), value)
            .finally(Keyword::Null)
            .into()
    };
    identity
        .into_table(table_of(target, Entities::Table))
        .columns([Entities::Id].into_iter().chain(current_state).chain(evolution))
        .select_from(
            Query::select()
                .expr(projected(Entities::Id))
                .expr(Expr::val(accepted_at))
                .expr(Expr::val(accepted_at))
                .expr(projected(Entities::ExpiresAt))
                .expr(projected(Entities::MinAttributeExpiresAt))
                .expr(established(Expr::val(accepted_at)))
                .expr(established(Expr::val(accepted_at)))
                .expr(established(projected(EntitySearch::EntityTypeNameKeys)))
                .from(Proj)
                .take(),
        )?;
    identity
        .on_conflict(
            OnConflict::column(Entities::Id)
                .update_columns(current_state)
                .value(Entities::WriteCount, next_write_count(Entities::Table))
                .action_and_where(Expr::col((Entities::Table, Entities::CreatedAt)).is_null())
                .to_owned(),
        )
        .returning(
            Query::returning().exprs([
                Expr::col((New, Entities::EntityKey)),
                Expr::col((New, Entities::Id)),
                Expr::col((Old, Entities::EntityKey))
                    .is_null()
                    .and(Expr::col((New, Entities::EvolutionCreatedAt)).is_not_null()),
            ]),
        );
    Ok(identity)
}

impl EntityRepository {
    /// Insert every entity in `entities` that does not already exist, with the
    /// attribute instance rows `rows` stages for it, returning the entities
    /// actually inserted. Each is created and modified at `accepted_at`, the
    /// instant its attributes carry (clause 4.8), so every entity one commit
    /// writes shares it. A new identity row of an entity in `establishing` is
    /// minted with its Temporal Evolution too ([`entity_insert_statement`]),
    /// and those come back as established.
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
        accepted_at: DateTime<Utc>,
        establishing: &HashSet<Identifier>,
    ) -> Result<CreatedEntities, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(CreatedEntities::default());
        }
        types_present(entities)?;

        let rows = entity_insert_statement(target, entities, rows, accepted_at, establishing)?
            .into_query_as::<(String, bool)>()
            .fetch_all(&mut **tx);
        let rows = observe_statement(ENTITY_INSERT, rows).await.map_err(entity_write_failure)?;

        let mut created = CreatedEntities::default();
        for (id, established) in &rows {
            let id = stored_uri(id)?;
            if *established {
                // clone: the identifier is both landed and established.
                created.established.push(id.clone());
            }
            created.landed.push(id);
        }
        Ok(created)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        current_attribute::{staged_rows::StagedRows, staging::ROW_COLUMNS},
        entity::{
            params::PreparedEntityColumns,
            write::{insert::entity_insert_statement, placeholders, staging::insert_staging_columns},
        },
        statement_text::text_of_query,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::{DateTime, Utc};
    use std::collections::{BTreeSet, HashSet};

    /// The instant every rendered insert is accepted at.
    fn accepted_at() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().expect("a valid instant")
    }

    /// The rendered insert of no entity, unwrapped for assertion.
    fn entity_insert_sql(target: &ContextTarget) -> String {
        let entities = PreparedEntityColumns::with_capacity(0);
        text_of_query(
            entity_insert_statement(target, &entities, StagedRows::default(), accepted_at(), &HashSet::new())
                .expect("the fixed insert builds")
                .into_query(),
        )
    }

    /// The rendered insert, unwrapped for assertion.
    fn insert() -> String {
        entity_insert_sql(&ContextTarget::Live)
    }

    #[test]
    fn the_insert_binds_contiguous_placeholders_and_names_its_staging_columns() {
        let sql = insert();
        let binds = insert_staging_columns().len() + ROW_COLUMNS.len() + 1 + 4;
        pretty_assertions::assert_eq!(
            placeholders(&sql),
            (1..=u32::try_from(binds).expect("a small count")).collect::<BTreeSet<u32>>()
        );
        assert!(
            sql.contains(
                " AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, verbatim_scope_paths_json, loc, loc_reach, loc_expires_at, expires_at, min_attribute_expires_at, establishes_evolution)"
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
        assert!(sql.contains(r"FROM UNNEST($16::text[]"), "{sql}");
        assert!(sql.contains(r"$46::sub_attribute_tree[], $40::text[]) AS r("), "{sql}");
        assert!(sql.contains(r#""r"."sub_attributes" AS "sub_attributes""#), "{sql}");
        assert!(sql.contains(r#""r"."sub_eq_keys"::bigint[] AS "sub_eq_keys""#), "{sql}");
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
            sql.contains(r#""ins" ("entity_key", "id", "established") AS (INSERT INTO "entities" ("id", "created_at", "modified_at", "expires_at", "min_attribute_expires_at", "evolution_created_at", "evolution_modified_at", "evolution_type_name_keys")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""srch" ("entity_key", "new_types", "new_instance_names", "new_instance_kinds") AS (INSERT INTO "entity_search" ("entity_key", "location_expires_at", "entity_type_name_keys", "attribute_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds", "scope_paths", "verbatim_scope_paths", "location", "location_reach")"#), "{sql}");
        assert!(
            sql.contains(r#"RETURNING "entity_key", "entity_type_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds")"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"SELECT "id", "established" FROM "ins""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches("ST_MakeValid").count(),
            2,
            "the hoisted geometry is validated once, in the projection, and each instance geometry is repaired once, beside its written form in the row staging: {sql}"
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
    /// arrays, once as label paths and once as written, and stay NULL for an
    /// entity without a Scope.
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
            sql.contains(r#"(CASE WHEN ("d"."scope_paths_json" IS NOT NULL) THEN ARRAY (SELECT jsonb_array_elements_text("d"."scope_paths_json")::ltree) ELSE NULL END) AS "scope_paths""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(CASE WHEN ("d"."verbatim_scope_paths_json" IS NOT NULL) THEN ARRAY (SELECT jsonb_array_elements_text("d"."verbatim_scope_paths_json")::text) ELSE NULL END) AS "verbatim_scope_paths""#),
            "{sql}"
        );
        assert!(sql.contains(r#""r"."object_type_keys"::integer[] AS "object_type_keys""#), "{sql}");
    }

    #[test]
    fn the_insert_reads_every_geometry_from_its_embedded_srid() {
        let sql = insert();
        assert!(sql.contains(r#"ST_MakeValid(ST_GeomFromEWKB("d"."loc"))"#), "{sql}");
        assert!(sql.contains(r#"ST_GeomFromEWKB("d"."loc_reach") AS "location_reach""#), "{sql}");
        assert!(
            sql.contains(r#", ST_GeomFromEWKB("r"."value_geometry_reach") AS "value_geometry_reach""#),
            "{sql}"
        );
        assert!(sql.contains(r#", ST_GeomFromEWKB("r"."value_geometry") AS "value_geometry""#), "{sql}");
        assert!(sql.contains(r#"(CASE WHEN (ST_IsValid(ST_GeomFromEWKB("r"."value_geometry"))) THEN NULL ELSE ST_MakeValid(ST_GeomFromEWKB("r"."value_geometry")) END) AS "value_geometry_valid""#), "{sql}");
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }

    /// The entity's own `createdAt` and `modifiedAt` are the instant the
    /// commit was accepted at, the one its attributes carry (clause 4.8), so a
    /// batch committed in several transactions stamps one instant (clause
    /// 5.5.11.0). An identity row holding a current state is left alone, and
    /// one holding the Temporal Evolution alone (clause 5.6.6 deleted its
    /// current state) is given one and returned, so clause 5.6.1 creates the
    /// Entity anew under the same key, one write count past the deletion. The
    /// conflict action leaves the evolution columns alone, so only a row the
    /// insert minted carries the evolution it was asked to establish.
    #[test]
    fn the_insert_keeps_its_conflict_clause_and_revives_an_evolution_alone() {
        let sql = insert();
        assert!(
            sql.contains(concat!(
                r#"SELECT "proj"."id", $12, $13, "proj"."expires_at", "proj"."min_attribute_expires_at", "#,
                r#"(CASE WHEN ("proj"."establishes_evolution") THEN $14 ELSE NULL END), (CASE WHEN ("proj"."establishes_evolution") THEN $15 ELSE NULL END), "#,
                r#"(CASE WHEN ("proj"."establishes_evolution") THEN "proj"."entity_type_name_keys" ELSE NULL END) FROM "proj" "#,
                r#"ON CONFLICT ("id") DO UPDATE SET "created_at" = "excluded"."created_at", "modified_at" = "excluded"."modified_at", "#,
                r#""expires_at" = "excluded"."expires_at", "min_attribute_expires_at" = "excluded"."min_attribute_expires_at", "#,
                r#""write_count" = "entities"."write_count" + 1 WHERE "entities"."created_at" IS NULL "#,
                r#"RETURNING "new"."entity_key", "new"."id", "old"."entity_key" IS NULL AND "new"."evolution_created_at" IS NOT NULL)"#
            )),
            "{sql}"
        );
        assert!(!sql.contains("geography"), "the geography is an index expression, never stored: {sql}");
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
    /// per type beside the search row that carries the same keys, on the
    /// Evolution's side too where the insert minted the row with its evolution.
    #[test]
    fn a_create_lands_its_type_memberships_in_the_same_statement() {
        let sql = insert();
        assert!(
            sql.contains(
                r#""mbrs" AS (INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id", "is_current_member", "is_evolution_member")"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""ins"."id", TRUE, "ins"."established" FROM "ins""#), "{sql}");
    }

    /// A Snapshot branch lands its rows in the Snapshot's own schema, so every
    /// target relation of the insert is schema-qualified.
    #[test]
    fn a_snapshot_branch_lands_in_the_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, id));
        let sql = entity_insert_sql(&target);
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
