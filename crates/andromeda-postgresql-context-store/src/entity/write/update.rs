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
        landed_entity::LandedEntity,
        mapper::stored_uri,
        params::PreparedEntityColumns,
        predated_entity::PredatedEntity,
        repo::EntityRepository,
        staged_geometry::{kept_column, staged_column},
        types_present::{entity_write_failure, types_present},
        write::{
            latest_instant::{LatestInstant, QueuedAt, queued_instant},
            search_image::SearchImage,
            staging::{
                HOISTED_LOCATION,
                ProjectedWrite,
                StagedColumn,
                entity_arrays,
                projected,
                projection_cte,
                staged,
                update_staging_columns,
                with_staged_alias,
            },
            statement_ctes::{Attrs, Dropped, Ents, Gone, Moved, Proj, Srch, Stamped},
            type_membership::{Mbrs, Strays, Unflagged, added_memberships, dropped_memberships},
            update_anchor::update_anchor,
            version_guard::expectation_arrays,
        },
        written_entities::WrittenEntities,
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
};
use andromeda_context_commit::{entity_version::EntityVersion, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::now,
    external_arguments::{ExternalArguments, ExternallyBound},
    generic_plan_stretch::GenericPlanStretch,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    returning_image::{New, Old},
    transaction_id::current_transaction_id,
    unnest::UnnestAliased,
};
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, CommonTableExpression, Cond, Expr, ExprTrait, IntoIden, Query, SimpleExpr, WithClause, WithQuery};
use sqlx::{Postgres, Transaction};

/// Build the batch update of `entities`, each held to its entry of
/// `expected` and modified at `accepted_at`, with the attribute
/// instance rows `rows` and the grains `grains` stage for them.
///
/// ```sql
/// WITH proj AS (SELECT …, <latest queued intent of d.id> AS queued_at FROM UNNEST($1::text[], …, $14::bigint[]) AS d(…)),
///      stamped AS (UPDATE entities SET write_count = CASE WHEN <refused> THEN entities.write_count ELSE entities.write_count + 1 END,
///                                      modified_at = CASE WHEN <refused> THEN entities.modified_at ELSE $accepted_at END,
///                                      expires_at = CASE WHEN <refused> THEN entities.expires_at ELSE proj.expires_at END, …
///               FROM proj
///               WHERE entities.id = proj.id
///                 AND (proj.expected_entity_key IS NULL
///                      OR (entities.entity_key = proj.expected_entity_key AND entities.write_count = proj.expected_write_count))
///               RETURNING entities.entity_key, entities.id,
///                         CASE WHEN new.write_count = old.write_count THEN <latest of old> END AS latest_instant),
///      ents AS (SELECT entity_key, id FROM stamped WHERE latest_instant IS NULL),
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
///      instances AS (SELECT r.id, r.attribute_name_key, … FROM UNNEST($15::text[], …) AS r(…)),
///      grains AS (SELECT g.id, g.attribute_name_key, g.dataset_key, g.ordinal FROM UNNEST($42::text[], …) AS g(…)),
///      gone AS (DELETE FROM current_attributes WHERE entity_key IN (SELECT entity_key FROM ents WHERE id = ANY($46::text[]))
///               AND (<the row's grain>) NOT IN (<every staged grain>)),
///      dropped AS (DELETE FROM current_attributes WHERE entity_key IN (<ents removing a grain>) AND ((<the row's grain>) IN (<the removed grains>)) IS TRUE),
///      moved AS (UPDATE current_attributes SET ordinal = ((<the moved grains' ordinals by grain>) ->> (<the row's grain>)::text)::integer
///                WHERE entity_key IN (<ents moving a grain>) AND ((<the row's grain>) IN (<the moved grains>)) IS TRUE),
///      attrs AS (INSERT INTO current_attributes (entity_key, …) SELECT ents.entity_key, instances.… FROM ents JOIN instances ON instances.id = ents.id
///                ON CONFLICT (entity_key, attribute_name_key, dataset_key) DO UPDATE SET ordinal = excluded.ordinal, …
///                WHERE (current_attributes.ordinal, …) IS DISTINCT FROM (excluded.ordinal, …)),
///      census_shapes AS (SELECT <image arrays>, SUM(±1) FROM srch … GROUP BY <image arrays>),
///      census_types AS (INSERT INTO entity_type_census …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT id, now(), latest_instant FROM stamped
/// ```
///
/// `<refused>` is `GREATEST(entities.modified_at, entities.evolution_modified_at,
/// proj.queued_at) > $accepted_at`: the Entity holds an instant later than the
/// write's.
///
/// Each written row comes back with `now()`, the transaction timestamp: the
/// store's own clock, which orders the change against a Subscription's
/// lifecycle the same way whichever broker committed either.
///
/// The identity row is the anchor ([`update_anchor`]): it counts the write,
/// stamps the clause 4.8 `modifiedAt` and replaces both expiries, which are
/// derived from the state just written: an entity whose new state no longer
/// carries `expiresAt` (clause 4.22) must not keep an expiry instant that would
/// later delete it. Its expectation columns are nullable so one statement carries both an
/// unconditional overwrite and a compare-and-set in the same batch: a null
/// expectation matches whatever is there, a non-null one matches only an
/// untouched row. An identifier that does not come back therefore means
/// either "gone" or "moved on", and which of those it is depends on the
/// precondition the caller attached. No column the anchor writes is in an
/// index unless it changed, so the update stays a heap-only tuple.
///
/// An Entity holding an instant later than `accepted_at` keeps every column
/// as it was, its write count included, and its row answers that instant: the
/// condition reads the row the anchor writes, after any writer it waited on.
/// It is not one of `ents`, so nothing else of the write lands.
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
    expected: &[Option<EntityVersion>],
    accepted_at: DateTime<Utc>,
) -> Result<ExternallyBound<'entities, UnnestAliased<WithQuery>>, ContextStoreError> {
    let columns = update_staging_columns();
    let queued = queued_instant(target, staged(StagedColumn::Id));
    let projected_queued = queued.is_some().then(|| projected(QueuedAt));
    let mut external = ExternalArguments::default();
    let mut staged_entities = entity_arrays(entities, &staged_column(&entities.locations), &mut external);
    staged_entities.push(Expr::val(kept_column(&entities.locations)));
    staged_entities.extend(expectation_arrays(expected));
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
    search.value(HOISTED_LOCATION.stored, hoisted_value(HOISTED_LOCATION.stored));
    search.value(HOISTED_LOCATION.reach_stored, hoisted_value(HOISTED_LOCATION.reach_stored));
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

    let dropped = dropped_memberships(target);
    let mut with = WithClause::new();
    with.cte(projection_cte(&columns, staged_entities, ProjectedWrite::Update { queued }))
        .cte(
            CommonTableExpression::new()
                .table_name(Stamped)
                .columns([Entities::EntityKey.into_iden(), Entities::Id.into_iden(), LatestInstant.into_iden()])
                .query(update_anchor(target, accepted_at, projected_queued))
                .to_owned(),
        )
        .cte(
            CommonTableExpression::new()
                .table_name(Ents)
                .query(
                    Query::select()
                        .columns([Entities::EntityKey, Entities::Id])
                        .from(Stamped)
                        .and_where(Expr::col(LatestInstant).is_null())
                        .take(),
                )
                .to_owned(),
        )
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
        .cte(CommonTableExpression::new().table_name(Unflagged).query(dropped.cleared).to_owned())
        .cte(CommonTableExpression::new().table_name(Strays).query(dropped.deleted).to_owned())
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
    let statement = with.query(Query::select().column(Entities::Id).expr(now()).column(LatestInstant).from(Stamped).take());

    let statement = with_grains_alias(with_instances_alias(with_staged_alias(UnnestAliased::new(statement), &columns)));
    Ok(ExternallyBound::new(statement, external))
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

/// The value a rewritten search row takes for one column of the hoisted
/// geometry, the geometry or its reach:
/// `CASE WHEN proj.loc_kept THEN entity_search.<column> ELSE proj.<column> END`.
///
/// A kept geometry is the stored column itself, so a row rewritten for another
/// column keeps the geometry's stored value rather than a re-projection of it.
fn hoisted_value(column: EntitySearch) -> SimpleExpr {
    CaseStatement::new()
        .case(projected(HOISTED_LOCATION.kept), Expr::col((EntitySearch::Table, column)))
        .finally(projected(column))
        .into()
}

/// Whether the hoisted geometry changes the search row:
/// `NOT proj.loc_kept AND (entity_search.location IS DISTINCT FROM
/// proj.location OR entity_search.location_reach IS DISTINCT FROM
/// proj.location_reach)`.
///
/// Two written geometries can repair to the same stored one and still bow
/// differently, so the reach is compared on its own.
fn hoisted_changed() -> Cond {
    let differs = |column: EntitySearch| SimpleExpr::from(Expr::col((EntitySearch::Table, column))).distinct_from(projected(column));
    Cond::all()
        .add(projected(HOISTED_LOCATION.kept).not())
        .add(Cond::any().add(differs(HOISTED_LOCATION.stored)).add(differs(HOISTED_LOCATION.reach_stored)))
}

impl EntityRepository {
    /// Write every entity in `entities` whose expectation still holds and that
    /// holds no instant later than `accepted_at`, its attribute instances
    /// edited as `rows` and `grains` stage it, returning the entities actually
    /// written with the instant of the transaction that wrote them, and every
    /// entity refused for holding a later instant, with that instant.
    ///
    /// `expected` is positionally aligned with `entities`; a `None`
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
        expected: &[Option<EntityVersion>],
        accepted_at: DateTime<Utc>,
    ) -> Result<WrittenEntities<LandedEntity>, ContextStoreError> {
        if entities.ids.is_empty() {
            return Ok(WrittenEntities::none());
        }
        if expected.len() != entities.ids.len() {
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Update,
                expectations: expected.len(),
                rows: entities.ids.len(),
            }
            .into());
        }
        types_present(entities)?;

        // The statement reads the written entities through `UNNEST` of bound arrays.
        let stretch = GenericPlanStretch::begin(tx).await.map_err(entity_write_failure)?;
        let rows = entity_update_statement(target, entities, rows, grains, expected, accepted_at)?
            .into_query_as::<(String, DateTime<Utc>, Option<DateTime<Utc>>)>()
            .fetch_all(&mut **tx);
        let rows = observe_statement(ObservedStatement::EntityUpdate, rows).await.map_err(entity_write_failure)?;
        stretch.end(tx).await.map_err(entity_write_failure)?;

        let mut written = WrittenEntities::none();
        for (id, committed_at, latest) in &rows {
            let id = stored_uri(id)?;
            match latest {
                Some(latest) => written.predated.push(PredatedEntity {
                    id,
                    latest: SystemInstant::new(*latest),
                }),
                None => written.landed.push(LandedEntity {
                    id,
                    committed_at: StoreInstant::new(*committed_at),
                }),
            }
        }
        Ok(written)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
    use chrono::{DateTime, Utc};
    use std::collections::BTreeSet;

    /// The instant every rendered update is accepted at.
    fn accepted_at() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().expect("a valid instant")
    }

    /// The rendered update of no entity on `target`.
    fn update_on(target: &ContextTarget) -> String {
        let entities = PreparedEntityColumns::with_capacity(0);
        text_of_query(
            entity_update_statement(target, &entities, StagedRows::default(), StagedGrains::default(), &[], accepted_at())
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
        // The write instant binds once as the stamp and once in the refusal
        // each of the four columns the anchor sets tests.
        let binds = update_staging_columns().len() + ROW_COLUMNS.len() + 1 + 5 + 5;
        pretty_assertions::assert_eq!(
            placeholders(&sql),
            (1..=u32::try_from(binds).expect("a small count")).collect::<BTreeSet<u32>>()
        );
        assert!(
            sql.contains(
                " AS d(id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, verbatim_scope_paths_json, loc, loc_reach, loc_expires_at, expires_at, min_attribute_expires_at, loc_kept, expected_entity_key, expected_write_count)"
            ),
            "{sql}"
        );
        assert!(
            !sql.to_lowercase().contains("ordinality"),
            "no row is paired by its position in the batch: {sql}"
        );
    }

    /// The compare-and-set version is the row's key and write count, which
    /// the anchor compares with what the caller read and moves by one; the
    /// clause 4.8 `modifiedAt` is stamped with the instant the commit was
    /// accepted at, the one its attributes carry, and is not the version: two
    /// writes accepted in the same microsecond stamp the same one.
    #[test]
    fn a_compare_and_set_write_states_the_read_version() {
        let sql = update();
        assert!(
            sql.contains(r#""stamped" ("entity_key", "id", "latest_instant") AS (UPDATE "entities" SET "write_count" = (CASE WHEN"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM "proj" WHERE "entities"."id" = "proj"."id" AND "entities"."created_at" IS NOT NULL AND ("proj"."expected_entity_key" IS NULL OR ("entities"."entity_key" = "proj"."expected_entity_key" AND "entities"."write_count" = "proj"."expected_write_count"))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""ents" AS (SELECT "entity_key", "id" FROM "stamped" WHERE "latest_instant" IS NULL)"#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"SELECT "id", NOW(), "latest_instant" FROM "stamped""#), "{sql}");
    }

    /// Clause 4.8: the anchor refuses a write stamped before the latest
    /// instant the Entity holds, its current state's, its Temporal
    /// Evolution's or its latest queued intent's, by keeping every column it
    /// sets, write count included; the row then answers that instant and is
    /// none of the rows every other relation is written for.
    #[test]
    fn the_anchor_refuses_a_write_stamped_before_the_entity_s_latest_instant() {
        let sql = update();
        let refused = r#"GREATEST("entities"."modified_at", "entities"."evolution_modified_at", "proj"."queued_at") > $"#;
        pretty_assertions::assert_eq!(sql.matches(refused).count(), 4, "{sql}");
        for (column, written) in [
            ("write_count", r#""entities"."write_count" + 1"#),
            ("modified_at", "$16"),
            ("expires_at", r#""proj"."expires_at""#),
            ("min_attribute_expires_at", r#""proj"."min_attribute_expires_at""#),
        ] {
            assert!(
                sql.contains(&format!(r#"THEN "entities"."{column}" ELSE {written} END)"#)),
                "{column} keeps its value when refused: {sql}"
            );
        }
        assert!(
            sql.contains(r#"RETURNING "entities"."entity_key", "entities"."id", (CASE WHEN ("new"."write_count" = "old"."write_count") THEN GREATEST("old"."modified_at", "old"."evolution_modified_at", "proj"."queued_at") ELSE NULL END))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "temporal_append_intents"."recorded_at" FROM "temporal_append_intents" WHERE "temporal_append_intents"."entity_id" = "d"."id" ORDER BY "temporal_append_intents"."intent_key" DESC LIMIT 1) AS "queued_at""#),
            "the latest queued intent is read once per row, in the projection: {sql}"
        );
    }

    /// A Snapshot branch queues no history, so its anchor reads the identity
    /// row's instants alone.
    #[test]
    fn a_snapshot_anchor_reads_no_queue() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = update_on(&snapshot);
        assert!(!sql.contains("temporal_append_intents"), "{sql}");
        assert!(!sql.contains("queued_at"), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("entities"."modified_at", "entities"."evolution_modified_at") > $"#),
            "{sql}"
        );
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
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_key") DO UPDATE SET"#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#""moved" AS (UPDATE "current_attributes" SET "ordinal" = ((SELECT jsonb_object_agg(("ents"."entity_key", "grains"."attribute_name_key""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#""grains"."dataset_key" FROM "ents" JOIN "grains" ON "grains"."id" = "ents"."id" WHERE "grains"."ordinal" IS NOT NULL)) IS TRUE)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""dropped" AS (DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "ents"."entity_key" FROM "ents" JOIN (SELECT DISTINCT "grains"."id" FROM "grains""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"IN (SELECT "ents"."entity_key", "grains"."attribute_name_key", "grains"."dataset_key" FROM "ents" JOIN "grains" ON "grains"."id" = "ents"."id" WHERE "grains"."ordinal" IS NULL)) IS TRUE)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT IN (SELECT "ents"."entity_key", "instances"."attribute_name_key", "instances"."dataset_key" FROM "instances" JOIN "ents""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""gone" AS (DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "entity_key" FROM "ents" WHERE "id" = ANY($47::text[]))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r"$43::text[], $44::integer[], $45::text[], $46::integer[]) AS g(id, attribute_name_key, dataset_key, ordinal)"),
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
            sql.contains(r#""location_reach" = (CASE WHEN ("proj"."loc_kept") THEN "entity_search"."location_reach" ELSE "proj"."location_reach" END)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"((NOT "proj"."loc_kept") AND (("entity_search"."location" IS DISTINCT FROM "proj"."location") OR ("entity_search"."location_reach" IS DISTINCT FROM "proj"."location_reach")))"#),
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
        pretty_assertions::assert_eq!(search.matches("IS DISTINCT FROM").count(), 9, "nine search columns are guarded: {sql}");
        for column in [
            "entity_type_name_keys",
            "attribute_name_keys",
            "attribute_instance_name_keys",
            "attribute_instance_kinds",
            "scope_paths",
            "verbatim_scope_paths",
            "location",
            "location_reach",
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
    fn the_projection_validates_the_hoisted_geometry_and_repairs_each_instance_geometry_once() {
        let sql = update();
        pretty_assertions::assert_eq!(sql.matches("ST_MakeValid").count(), 2, "{sql}");
        assert!(sql.contains(r#"ST_MakeValid(ST_GeomFromEWKB("d"."loc"))"#), "{sql}");
        assert!(sql.contains(r#"(CASE WHEN (ST_IsValid(ST_GeomFromEWKB("r"."value_geometry"))) THEN NULL ELSE ST_MakeValid(ST_GeomFromEWKB("r"."value_geometry")) END) AS "value_geometry_valid""#), "{sql}");
        assert!(sql.starts_with(r#"WITH "proj" AS ("#), "{sql}");
    }
}
