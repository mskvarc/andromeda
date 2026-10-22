//! The batch update: one statement that writes each staged entity's edited
//! attribute instances and rewrites its search and lifecycle rows.

use crate::{
    current_attribute::{
        staged_grains::{GrainArrays, StagedGrains, grains_cte, with_grains_alias},
        staged_rows::StagedRows,
        staging::{instances_cte, with_instances_alias},
        write::{dropped_rows, moved_rows, vanished_rows, written_rows},
    },
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_uri,
        params::PreparedEntityColumns,
        repo::EntityRepository,
        staged_geometry::{kept_column, staged_column},
        types_present::{entity_write_failure, types_present},
        write::{
            search_image::SearchImage,
            staging::{
                Attrs,
                Dropped,
                Ents,
                Gone,
                HOISTED_LOCATION,
                Moved,
                Proj,
                ProjectedWrite,
                Srch,
                StagedColumn,
                entity_arrays,
                expectation,
                projected,
                projection_cte,
                update_staging_columns,
                with_staged_alias,
            },
            type_membership::{Mbrs, Strays, added_memberships, dropped_memberships},
        },
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::now,
    external_arguments::{ExternalArguments, ExternallyBound},
    pg_array::nullable_array,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    returning_image::{New, Old},
    transaction_id::current_transaction_id,
    unnest::UnnestAliased,
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, CommonTableExpression, Cond, Expr, ExprTrait, IntoIden, Query, SimpleExpr, UpdateStatement, WithClause, WithQuery};
use sqlx::{Postgres, Transaction};

/// The statement name the entity set-update is observed under.
const ENTITY_UPDATE: StatementName = StatementName("entity-update");

/// Build the batch update of `entities`, each held to its entry of
/// `expected_modified_at`, with the attribute instance rows `rows` and the
/// grains `grains` stage for them.
///
/// ```sql
/// WITH proj AS (SELECT … FROM UNNEST($1::text[], …, $13::timestamptz[]) AS d(…)),
///      ents AS (UPDATE entities SET modified_at = NOW(), expires_at = proj.expires_at,
///                                   min_attribute_expires_at = proj.min_attribute_expires_at
///               FROM proj
///               WHERE entities.id = proj.id
///                 AND (proj.expected_modified_at IS NULL OR entities.modified_at = proj.expected_modified_at)
///               RETURNING entities.entity_key, entities.id),
///      srch(entity_key, id, old_types, new_types, old_instance_names, …) AS
///              (UPDATE entity_search SET entity_type_name_keys = proj.entity_type_name_keys, …,
///                      location = CASE WHEN proj.loc_kept THEN entity_search.location ELSE proj.location END, …,
///                      written_xid = pg_current_xact_id() FROM ents, proj
///               WHERE proj.id = ents.id AND entity_search.entity_key = ents.entity_key
///                 AND (entity_search.entity_type_name_keys IS DISTINCT FROM proj.entity_type_name_keys OR …
///                      OR (NOT proj.loc_kept AND entity_search.location IS DISTINCT FROM proj.location) OR … )
///               RETURNING entity_search.entity_key, ents.id, old.entity_type_name_keys, new.entity_type_name_keys, …),
///      strays AS (DELETE FROM entity_type_members WHERE (entity_type_name_key, id) IN
///                 (SELECT <old type the new row lacks>, srch.id FROM srch …)),
///      mbrs AS (INSERT INTO entity_type_members (…) SELECT srch.entity_key, <new type the old row lacked>, srch.id FROM srch …),
///      instances AS (SELECT r.id, r.attribute_name_key, … FROM UNNEST($14::text[], …) AS r(…)),
///      grains AS (SELECT g.id, g.attribute_name_key, g.dataset_id, g.ordinal FROM UNNEST($41::text[], …) AS g(…)),
///      gone AS (DELETE FROM current_attributes WHERE entity_key IN (SELECT entity_key FROM ents WHERE id = ANY($45::text[]))
///               AND NOT EXISTS (<a staged row of the same grain>)),
///      dropped AS (DELETE FROM current_attributes WHERE entity_key IN (<ents removing a grain>) AND EXISTS (<a removed grain of the row>)),
///      moved AS (UPDATE current_attributes SET ordinal = grains.ordinal FROM ents, grains WHERE grains.ordinal IS NOT NULL AND <the row's grain>),
///      attrs AS (INSERT INTO current_attributes (entity_key, …) SELECT ents.entity_key, instances.… FROM ents JOIN instances ON instances.id = ents.id
///                ON CONFLICT (entity_key, attribute_name_key, dataset_id) DO UPDATE SET ordinal = excluded.ordinal, …
///                WHERE (current_attributes.ordinal, …) IS DISTINCT FROM (excluded.ordinal, …)),
///      census_shapes AS (SELECT <image arrays>, SUM(±1) FROM srch … GROUP BY <image arrays>),
///      census_types AS (INSERT INTO entity_type_census …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT id FROM ents
/// ```
///
/// The identity row is the anchor ([`entity_write`]): it stamps the clause
/// 4.8 `modifiedAt` and replaces both expiries, which are derived from the
/// state just written: an entity whose new state no longer carries `expiresAt`
/// (clause 4.22) must not keep an expiry instant that would later delete it.
/// Its expectation column is nullable so one statement carries both an
/// unconditional overwrite and a compare-and-set in the same batch: a null
/// expectation matches whatever is there, a non-null one matches only an
/// untouched row. An identifier that does not come back therefore means
/// either "gone" or "moved on", and which of those it is depends on the
/// precondition the caller attached. No column the anchor writes is in an
/// index unless it changed, so the update stays a heap-only tuple.
///
/// Every other relation is written only for the entities whose anchor landed.
/// The attribute instances are written as each entity's edit says
/// ([`RowEdit`](crate::current_attribute::row_edit::RowEdit)): a staged row
/// updates its stored grain (clause 4.5.5) in place, keeping its `createdAt`,
/// or is inserted when the grain is new; a staged grain with an ordinal moves
/// its row there and a staged grain without one deletes its row; and an entity
/// whose staged rows are every row it keeps, named in the overwritten list,
/// loses every stored grain none of them has. A row the edit does not name is
/// not touched. The search row is rewritten only where a projected column
/// actually differs from what is stored: `IS DISTINCT FROM` on every column,
/// so a value-only write leaves it and every search index untouched. A geometry
/// the write leaves as the stored row holds it is staged as kept, with no EWKB:
/// the statement neither parses nor validates nor compares its positions, and a
/// row rewritten for another column keeps the stored geometry. The type
/// memberships and the discovery census move with the search row: both are
/// derived from the pre- and post-image its write returns (PostgreSQL 18
/// `RETURNING old.*, new.*`), which is the row the write actually replaced,
/// found after any concurrent writer it waited on. A value-only write rewrites
/// no search row, so it moves no membership and counts nothing.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the membership or row
/// insert ever stops matching its column list, which their fixed shapes make
/// unreachable.
pub(super) fn entity_update_statement<'entities>(
    target: &ContextTarget,
    entities: &'entities PreparedEntityColumns,
    rows: StagedRows<'_>,
    grains: StagedGrains<'_>,
    expected_modified_at: &[Option<DateTime<Utc>>],
) -> Result<ExternallyBound<'entities, UnnestAliased<WithQuery>>, ContextStoreError> {
    let columns = update_staging_columns();
    let mut external = ExternalArguments::default();
    let mut staged_entities = entity_arrays(entities, staged_column(&entities.locations), &mut external);
    staged_entities.push(Expr::val(kept_column(&entities.locations)));
    staged_entities.push(nullable_array(expected_modified_at.iter().copied()));
    let instance_arrays = rows.into_arrays(&mut external);
    let GrainArrays {
        grains: grain_arrays,
        overwritten,
    } = grains.into_arrays();

    let mut search = Query::update();
    search.table(table_of(target, EntitySearch::Table));
    for column in SCALAR_SEARCH_COLUMNS {
        search.value(column, projected(column));
    }
    search.value(HOISTED_LOCATION.stored, hoisted_value());
    // A rewritten search row is a changed type, attribute name, scope or
    // geometry, which a spatial index catching up from the store must find.
    search.value(EntitySearch::WrittenXid, current_transaction_id());
    let scalar_changed = SCALAR_SEARCH_COLUMNS.into_iter().fold(Cond::any(), |changed, column| {
        changed.add(SimpleExpr::from(Expr::col((EntitySearch::Table, column))).distinct_from(projected(column)))
    });
    let changed = scalar_changed.add(hoisted_changed());
    search.from(Ents).from(Proj).cond_where(
        Cond::all()
            .add(Expr::col((Proj, Entities::Id)).equals((Ents, Entities::Id)))
            .add(Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Ents, Entities::EntityKey)))
            .add(changed),
    );
    search.returning(Query::returning().exprs([
        Expr::col((EntitySearch::Table, EntitySearch::EntityKey)),
        Expr::col((Ents, Entities::Id)),
        Expr::col((Old, EntitySearch::EntityTypeNameKeys)),
        Expr::col((New, EntitySearch::EntityTypeNameKeys)),
        Expr::col((Old, EntitySearch::AttributeInstanceNameKeys)),
        Expr::col((New, EntitySearch::AttributeInstanceNameKeys)),
        Expr::col((Old, EntitySearch::AttributeInstanceKinds)),
        Expr::col((New, EntitySearch::AttributeInstanceKinds)),
    ]));

    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, staged_entities, ProjectedWrite::Update))
        .cte(CommonTableExpression::new().table_name(Ents).query(entity_write(target)).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(Srch)
                .columns([
                    EntitySearch::EntityKey.into_iden(),
                    Entities::Id.into_iden(),
                    SearchImage::OldTypes.into_iden(),
                    SearchImage::NewTypes.into_iden(),
                    SearchImage::OldInstanceNames.into_iden(),
                    SearchImage::NewInstanceNames.into_iden(),
                    SearchImage::OldInstanceKinds.into_iden(),
                    SearchImage::NewInstanceKinds.into_iden(),
                ])
                .query(search)
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Strays).query(dropped_memberships(target)).to_owned())
        .cte(CommonTableExpression::new().table_name(Mbrs).query(added_memberships(target)?).to_owned())
        .cte(instances_cte(instance_arrays))
        .cte(grains_cte(grain_arrays))
        .cte(
            CommonTableExpression::new()
                .table_name(Gone)
                .query(vanished_rows(target, Ents, cast(overwritten, PostgresType::TextArray)))
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Dropped).query(dropped_rows(target, Ents)).to_owned())
        .cte(CommonTableExpression::new().table_name(Moved).query(moved_rows(target, Ents)).to_owned())
        .cte(CommonTableExpression::new().table_name(Attrs).query(written_rows(target, Ents)?).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Replaced)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(Query::select().column(Entities::Id).from(Ents).take());

    let statement = with_grains_alias(with_instances_alias(with_staged_alias(UnnestAliased::new(statement), &columns)));
    Ok(ExternallyBound::new(statement, external))
}

/// The update's anchor, `ents`: each identity row whose expectation still
/// holds, stamped and given its new expiries, with the key and identifier of
/// every row it landed in.
///
/// It finds each row through the unique identifier index, so its cost grows
/// with the batch whatever row estimate the plan was made for.
fn entity_write(target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, Entities::Table))
        .value(Entities::ModifiedAt, now())
        .value(Entities::ExpiresAt, projected(Entities::ExpiresAt))
        .value(Entities::MinAttributeExpiresAt, projected(Entities::MinAttributeExpiresAt))
        .from(Proj)
        .cond_where(
            Cond::all()
                .add(Expr::col((Entities::Table, Entities::Id)).equals((Proj, Entities::Id)))
                .add(expectation(Expr::col((Proj, StagedColumn::ExpectedModifiedAt)), Entities::Table)),
        )
        .returning(Query::returning().exprs([Expr::col((Entities::Table, Entities::EntityKey)), Expr::col((Entities::Table, Entities::Id))]))
        .take()
}

/// The seven scalar `entity_search` columns the update projects, each written
/// and guarded by the same list so no column can be rewritten without being
/// compared, or compared without being written. The hoisted geometry
/// ([`HOISTED_LOCATION`]) is written and guarded the same way, unless the row
/// keeps it.
const SCALAR_SEARCH_COLUMNS: [EntitySearch; 7] = [
    EntitySearch::EntityTypeNameKeys,
    EntitySearch::AttributeNameKeys,
    EntitySearch::AttributeInstanceNameKeys,
    EntitySearch::AttributeInstanceKinds,
    EntitySearch::ScopePaths,
    EntitySearch::VerbatimScopePaths,
    EntitySearch::LocationExpiresAt,
];

/// The value a rewritten search row takes for the hoisted geometry:
/// `CASE WHEN proj.loc_kept THEN entity_search.location ELSE proj.location END`.
///
/// A kept geometry is the stored column itself, so a row rewritten for another
/// column keeps the geometry's stored value rather than a re-projection of it.
fn hoisted_value() -> SimpleExpr {
    CaseStatement::new()
        .case(projected(HOISTED_LOCATION.kept), Expr::col((EntitySearch::Table, HOISTED_LOCATION.stored)))
        .finally(projected(HOISTED_LOCATION.stored))
        .into()
}

/// Whether the hoisted geometry changes the search row:
/// `NOT proj.loc_kept AND entity_search.location IS DISTINCT FROM proj.location`.
fn hoisted_changed() -> Cond {
    Cond::all()
        .add(projected(HOISTED_LOCATION.kept).not())
        .add(SimpleExpr::from(Expr::col((EntitySearch::Table, HOISTED_LOCATION.stored))).distinct_from(projected(HOISTED_LOCATION.stored)))
}

impl EntityRepository {
    /// Write every entity in `entities` whose expectation still holds, its
    /// attribute instances edited as `rows` and `grains` stage it, returning
    /// the entities actually written.
    ///
    /// `expected_modified_at` is positionally aligned with `entities`; a `None`
    /// entry overwrites unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the expectation slice is not the same
    /// length as `entities`, when an entity carries no type, when the update
    /// statement cannot be built, when the statement fails, or when a returned
    /// identifier is not a valid URI.
    pub async fn save_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        entities: &PreparedEntityColumns,
        rows: StagedRows<'_>,
        grains: StagedGrains<'_>,
        expected_modified_at: &[Option<DateTime<Utc>>],
    ) -> Result<Vec<Identifier>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected_modified_at.len() != entities.ids.len() {
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Update,
                expectations: expected_modified_at.len(),
                rows: entities.ids.len(),
            }
            .into());
        }
        types_present(entities)?;

        let rows = entity_update_statement(target, entities, rows, grains, expected_modified_at)?
            .into_query_as::<(String,)>()
            .fetch_all(&mut **tx);
        let rows = observe_statement(ENTITY_UPDATE, rows).await.map_err(entity_write_failure)?;

        rows.iter().map(|(id,)| stored_uri(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute::{staged_grains::StagedGrains, staged_rows::StagedRows, staging::ROW_COLUMNS},
        entity::{
            params::PreparedEntityColumns,
            write::{placeholders, staging::update_staging_columns, update::entity_update_statement},
        },
        statement_text::text_of_query,
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use std::collections::BTreeSet;

    /// The rendered update of no entity on `target`.
    fn update_on(target: &ContextTarget) -> String {
        let entities = PreparedEntityColumns::with_capacity(0);
        text_of_query(
            entity_update_statement(target, &entities, StagedRows::default(), StagedGrains::default(), &[])
                .expect("the fixed update builds")
                .into_query(),
        )
    }

    fn update() -> String {
        update_on(&ContextTarget::Live)
    }

    /// A Snapshot branch's update lands in the Snapshot's own schema.
    #[test]
    fn a_snapshot_update_names_the_branch_schema() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let text = update_on(&snapshot);
        for relation in ["entities", "entity_search", "current_attributes"] {
            assert!(text.contains(&format!(r#""snap_5_stg"."{relation}""#)), "{relation}: {text}");
        }
    }

    #[test]
    fn the_update_binds_contiguous_placeholders_and_names_its_staging_columns() {
        let sql = update();
        let binds = update_staging_columns().len() + ROW_COLUMNS.len() + 1 + 5;
        pretty_assertions::assert_eq!(
            placeholders(&sql),
            (1..=u32::try_from(binds).expect("a small count")).collect::<BTreeSet<u32>>()
        );
        assert!(
            sql.contains(
                " AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, verbatim_scope_paths_json, loc, loc_expires_at, expires_at, min_attribute_expires_at, loc_kept, expected_modified_at)"
            ),
            "{sql}"
        );
        assert!(
            !sql.to_lowercase().contains("ordinality"),
            "no row is paired by its position in the batch: {sql}"
        );
    }

    /// Clause 4.8: the compare-and-set token is the entity's `modifiedAt`, which
    /// the anchor compares with what the caller read and stamps anew.
    #[test]
    fn a_compare_and_set_write_states_the_read_token() {
        let sql = update();
        assert!(
            sql.contains(r#""ents" AS (UPDATE "entities" SET "modified_at" = NOW(), "expires_at" = "proj"."expires_at", "min_attribute_expires_at" = "proj"."min_attribute_expires_at" FROM "proj" WHERE "entities"."id" = "proj"."id" AND"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""proj"."expected_modified_at" IS NULL OR "entities"."modified_at" = "proj"."expected_modified_at""#),
            "{sql}"
        );
        assert!(sql.contains(r#"RETURNING "entities"."entity_key", "entities"."id")"#), "{sql}");
        assert!(sql.ends_with(r#"SELECT "id" FROM "ents""#), "{sql}");
    }

    /// The attribute instances are written as each entity's edit stages
    /// them, each row and grain paired with its entity by identifier and only
    /// for an entity the anchor wrote: staged rows upserted, moved rows given
    /// their ordinal, removed grains deleted, and an overwritten entity's
    /// unstated grains deleted.
    #[test]
    fn the_update_writes_the_edit_of_every_written_entity() {
        let sql = update();
        assert!(
            sql.contains(r#""attrs" AS (INSERT INTO "current_attributes" ("entity_key", "attribute_name_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#"FROM "ents" JOIN "instances" ON "instances"."id" = "ents"."id""#), "{sql}");
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_id") DO UPDATE SET"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""moved" AS (UPDATE "current_attributes" SET "ordinal" = "grains"."ordinal" FROM "ents", "grains""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""dropped" AS (DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "ents"."entity_key" FROM "ents" JOIN "grains""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""gone" AS (DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "entity_key" FROM "ents" WHERE "id" = ANY($40::text[]))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r"$36::text[], $37::integer[], $38::text[], $39::integer[]) AS g(id, attribute_name_key, dataset_id, ordinal)"),
            "{sql}"
        );
    }

    /// The batch's rows and grains find their entity by identifier, never by
    /// position, so the statement's binds do not grow with the batch and no
    /// plan depends on its size.
    #[test]
    fn a_batch_pairs_rows_by_entity_id() {
        let sql = update();
        assert!(!sql.to_lowercase().contains("ordinality"), "{sql}");
        assert!(sql.contains(r#""grains"."id" = "ents"."id""#), "{sql}");
        assert!(sql.contains(r#""instances"."id" = "ents"."id""#), "{sql}");
    }

    /// A geometry the row keeps is the stored column itself and changes
    /// nothing: the row is neither rewritten for it nor, when rewritten for
    /// another column, given a re-projection of it.
    #[test]
    fn a_kept_geometry_keeps_the_stored_column_and_is_not_compared() {
        let sql = update();
        assert!(
            sql.contains(r#""location" = (CASE WHEN ("proj"."loc_kept") THEN "entity_search"."location" ELSE "proj"."location" END)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"((NOT "proj"."loc_kept") AND ("entity_search"."location" IS DISTINCT FROM "proj"."location"))"#),
            "{sql}"
        );
    }

    /// Only the default `location` is hoisted; every other `GeoProperty` is
    /// matched through its attribute instance rows.
    #[test]
    fn the_update_hoists_no_space_geometry() {
        let sql = update();
        assert!(!sql.contains("observation_space"), "{sql}");
        assert!(!sql.contains("operation_space"), "{sql}");
    }

    #[test]
    fn the_search_row_is_rewritten_only_where_a_projected_column_differs() {
        let sql = update();
        assert!(
            sql.contains(r#""srch" ("entity_key", "id", "old_types", "new_types", "old_instance_names", "new_instance_names", "old_instance_kinds", "new_instance_kinds") AS (UPDATE "entity_search" SET "entity_type_name_keys" = "proj"."entity_type_name_keys""#),
            "{sql}"
        );
        let (search, _) = sql.split_once(r#""strays" AS"#).expect("the search write precedes the memberships");
        pretty_assertions::assert_eq!(search.matches("IS DISTINCT FROM").count(), 8, "eight search columns are guarded: {sql}");
        for column in [
            "entity_type_name_keys",
            "attribute_name_keys",
            "attribute_instance_name_keys",
            "attribute_instance_kinds",
            "scope_paths",
            "verbatim_scope_paths",
            "location",
            "location_expires_at",
        ] {
            assert!(
                sql.contains(&format!(r#""entity_search"."{column}" IS DISTINCT FROM "proj"."{column}""#)),
                "{column} must be guarded: {sql}"
            );
        }
        assert!(sql.contains(r#""entity_search"."entity_key" = "ents"."entity_key""#), "{sql}");
    }

    /// A spatial index catching up from the store reads the search rows written
    /// since its last read, so every rewrite of one must stamp the writing
    /// transaction; the stamp itself is not guarded, or it would rewrite rows
    /// no projected column changed.
    #[test]
    fn a_rewritten_search_row_is_stamped_with_the_writing_transaction() {
        let sql = update();
        assert!(sql.contains(r#""written_xid" = pg_current_xact_id() FROM "ents", "proj""#), "{sql}");
        assert!(!sql.contains(r#""written_xid" IS DISTINCT FROM"#), "{sql}");
    }

    /// Both expiries follow the state just written (clause 4.22): an update
    /// whose state dropped the entity's `expiresAt` clears the column, so the
    /// expiry sweep never deletes the entity at an instant its own state no
    /// longer asks for.
    #[test]
    fn the_anchor_replaces_both_expiries_with_the_written_states() {
        let sql = update();
        assert!(
            !sql.contains(r#"COALESCE("proj"."expires_at""#),
            "a dropped expiry must not fall back to the stored one: {sql}"
        );
        assert!(!sql.contains(r#"SET "id""#), "the identity is never rewritten: {sql}");
    }

    /// Clause 4.17 selects on membership, so an update that changes an
    /// Entity's types moves its membership rows in the same statement, from
    /// the images the search write returned.
    #[test]
    fn a_type_change_moves_the_type_memberships_in_the_same_statement() {
        let sql = update();
        assert!(sql.contains(r#""strays" AS (DELETE FROM "entity_type_members""#), "{sql}");
        assert!(sql.contains(r#""mbrs" AS (INSERT INTO "entity_type_members""#), "{sql}");
        pretty_assertions::assert_eq!(
            sql.matches(r#"JOIN "entity_search""#).count(),
            0,
            "no sibling reads the search relation's snapshot: {sql}"
        );
    }

    /// The search write returns both images of every row it rewrote, and the
    /// census counts the difference of the rows whose types or instances
    /// changed.
    #[test]
    fn the_search_write_returns_both_images_and_the_census_reads_them() {
        let sql = update();
        assert!(
            sql.contains(r#"RETURNING "entity_search"."entity_key", "ents"."id", "old"."entity_type_name_keys", "new"."entity_type_name_keys", "old"."attribute_instance_name_keys", "new"."attribute_instance_name_keys", "old"."attribute_instance_kinds", "new"."attribute_instance_kinds")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""census_shapes" AS (SELECT"#), "{sql}");
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
    }

    #[test]
    fn the_projection_validates_each_geometry_once() {
        let sql = update();
        pretty_assertions::assert_eq!(sql.matches("ST_MakeValid").count(), 2, "{sql}");
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
    }
}
