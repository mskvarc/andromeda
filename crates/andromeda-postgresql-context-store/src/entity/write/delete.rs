//! The batch delete: one statement that removes each staged entity whose
//! expectation still holds and hands back the system attributes it held.

use crate::{
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_uri,
        predated_entity::PredatedEntity,
        removed_entity::RemovedEntity,
        repo::EntityRepository,
        write::{
            latest_instant::{LatestInstant, latest_instant, latest_where_refused, predates},
            search_image::SearchImage,
            staging::{Staged, StagedColumn, delete_staging_columns, staging_source, with_staged_alias},
            statement_ctes::{Doomed, Srch},
            version_guard::{expectation, expectation_arrays, next_write_count},
        },
        written_entities::WrittenEntities,
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
};
use andromeda_context_commit::{deleted_entity::DeletedEntity, entity_version::EntityVersion, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_postgresql_branch::{schema_iden, table_of};
use andromeda_postgresql_query::{
    entity_sides::{holds_current_state, holds_evolution},
    type_membership::Memberships,
    type_membership_moves::side_departure,
};
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, E, Entities, EntitySearch},
    type_membership::MembershipSide,
};
use andromeda_postgresql_seaquery::{clock::now, pg_array::text_array, statement::Statement, unnest::UnnestAliased};
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{
    Alias,
    CommonTableExpression,
    Expr,
    ExprTrait,
    Func,
    FunctionCall,
    Iden,
    IntoIden,
    JoinType,
    Keyword,
    LockType,
    Query,
    SelectStatement,
    SimpleExpr,
    WithClause,
    WithQuery,
};
use sqlx::{Postgres, Row, Transaction};

/// The column the delete answers its transaction timestamp under.
const COMMITTED_AT: &str = "committed_at";

/// The delete CTE, `gone`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
struct Gone;

/// Whether a doomed Entity keeps its identity row, for its Temporal Evolution
/// or its queued history, `kept`, a column of `doomed`.
#[derive(Iden, Clone, Copy)]
#[iden = "kept"]
struct Kept;

/// The latest instant a doomed Entity holds, `latest`, a column of `doomed`.
#[derive(Iden, Clone, Copy)]
#[iden = "latest"]
struct Latest;

/// The CTE taking the current state off the identity rows a Temporal
/// Evolution keeps, `retired`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "retired"]
struct Retired;

/// The CTE removing the attribute instances of the identity rows a Temporal
/// Evolution keeps, `retired_attributes`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "retired_attributes"]
struct RetiredAttributes;

/// The CTE clearing the current flag of the memberships a Temporal Evolution
/// keeps, `unflagged`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "unflagged"]
struct Unflagged;

/// The CTE deleting the memberships no Temporal Evolution keeps, `strays`,
/// whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "strays"]
struct Strays;

/// Build the batch delete of `ids`, each held to its entry of `expected` and
/// stamped `accepted_at`.
///
/// ```sql
/// WITH doomed AS (SELECT e.entity_key, e.id, e.created_at, e.modified_at, e.expires_at,
///                        e.evolution_created_at IS NOT NULL OR e.deleted_or_queued_at IS NOT NULL AS kept,
///                        GREATEST(e.modified_at, e.evolution_modified_at, e.deleted_or_queued_at) AS latest
///                 FROM UNNEST(…) AS d(id, expected_entity_key, expected_write_count)
///                 JOIN entities e ON e.id = d.id
///                 WHERE e.created_at IS NOT NULL
///                   AND (d.expected_entity_key IS NULL
///                        OR (e.entity_key = d.expected_entity_key AND e.write_count = d.expected_write_count))
///                 FOR UPDATE OF e),
///      srch(entity_key, old_types, old_instance_names, old_instance_kinds) AS
///              (DELETE FROM entity_search WHERE entity_key IN (SELECT entity_key FROM doomed WHERE NOT latest > $accepted_at)
///               RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds),
///      gone AS (DELETE FROM entities WHERE entity_key IN (SELECT entity_key FROM doomed WHERE NOT latest > $accepted_at AND NOT kept)),
///      retired AS (UPDATE entities SET write_count = entities.write_count + 1, created_at = NULL, modified_at = NULL,
///                  expires_at = NULL, min_attribute_expires_at = NULL,
///                  deleted_or_queued_at = GREATEST(entities.deleted_or_queued_at, $accepted_at)
///                  WHERE entity_key IN (… WHERE NOT latest > $accepted_at AND kept)),
///      retired_attributes AS (DELETE FROM current_attributes WHERE entity_key IN (… AND kept)),
///      unflagged AS (UPDATE entity_type_members SET is_current_member = FALSE WHERE entity_key IN (… AND kept) AND is_evolution_member),
///      strays AS (DELETE FROM entity_type_members WHERE entity_key IN (… AND kept) AND NOT is_evolution_member),
///      census_types AS (INSERT INTO entity_type_census … FROM srch …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT id, created_at, modified_at, expires_at, now() AS committed_at,
///        CASE WHEN latest > $accepted_at THEN latest END AS latest_instant FROM doomed
/// ```
///
/// Each written row comes back with `now()`, the transaction timestamp: the
/// store's own clock, which orders the change against a Subscription's
/// lifecycle the same way whichever broker committed either.
///
/// Carries the same nullable expectation the update does, and for the same
/// reason: a delete computed from a state the caller read is a
/// read-modify-write like any other, and one that removed a state its author
/// never saw would be a lost update. The candidates are locked before the
/// delete so a concurrent write cannot slip between the expectation check and
/// the removal. An identity row the Temporal Evolution keeps counts the
/// deletion as a write, so a version read before it never matches the Entity
/// clause 5.6.1 creates anew on the same row.
///
/// An Entity without a Temporal Evolution keeps its identity row too while
/// its `deleted_or_queued_at` is set: its history may still wait in the queue,
/// and the drain brings the Evolution into being on this row. A create of the
/// identifier then lands on the row and is held after the deletion's instant
/// (clause 4.8), where a fresh row would be compared with nothing and could
/// take the history of the Entity the deletion removed. The column is NULL
/// for an Entity with no queued history and no Evolution, whose row goes.
///
/// The search row is deleted by name so that the rows it held come back: the
/// discovery census counts every one of their types and attribute instances
/// out, from the row the delete removed rather than from the statement's
/// snapshot of it. Clause 5.6.6 leaves the Temporal Evolution standing, so no
/// history relation is touched, and an Entity holding one keeps its identity
/// row with its current state taken off: its attribute instances go, and its
/// type memberships keep the Evolution's flag alone, none for a row without
/// one. Everything of an Entity whose row goes goes with it: the type memberships through
/// their cascading foreign key, the attribute instances through the
/// statement-level trigger on `entities`, which removes the instances of
/// every deleted entity in one pass. An Entity without a current state is not
/// a candidate at all, so a core delete of it finds nothing. The system
/// attributes come back because the clause 5.6.10 notification carries them
/// and cannot read them once the rows are gone; the attributes do not, because
/// every caller that needs them read them before the delete.
///
/// A candidate holding an instant later than `accepted_at` is locked and left
/// standing: the deletion's tombstones are recorded at `accepted_at`, and an
/// Evolution ending instances before it observed them would show them in
/// effect after the Entity is gone (clause 4.8). Its row answers that instant,
/// NULL for a candidate the delete removed. A row the delete keeps raises
/// `deleted_or_queued_at` to `accepted_at`: it clears `modified_at`, and
/// the tombstones state no type, so `evolution_modified_at` stays as it was
/// and the row would otherwise hold no instant of the deletion.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if a census ledger insert ever stops
/// matching its column list, which the fixed shapes make unreachable.
pub(super) fn entity_delete_statement(
    target: &ContextTarget,
    ids: &[Identifier],
    expected: &[Option<EntityVersion>],
    accepted_at: DateTime<Utc>,
) -> Result<UnnestAliased<WithQuery>, ContextStoreError> {
    let columns = delete_staging_columns();
    let ids = ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
    let mut arrays = vec![text_array(&ids)];
    arrays.extend(expectation_arrays(expected));
    let doomed = doomed_candidates(target, staging_source(&columns, arrays));

    let refused = || predates(Expr::col(Latest), accepted_at);
    let doomed_keys = || Query::select().column(Entities::EntityKey).from(Doomed).and_where(refused().not()).take();
    let doomed_keys_where = |kept: SimpleExpr| {
        Query::select()
            .column(Entities::EntityKey)
            .from(Doomed)
            .and_where(refused().not())
            .and_where(kept)
            .take()
    };
    let kept_keys = || doomed_keys_where(Expr::col(Kept));
    let unkept_keys = || doomed_keys_where(Expr::col(Kept).not());
    let search = Query::delete()
        .from_table(table_of(target, EntitySearch::Table))
        .and_where(Expr::col(EntitySearch::EntityKey).in_subquery(doomed_keys()))
        .returning(Query::returning().columns([
            EntitySearch::EntityKey,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
        ]))
        .take();
    let gone = Query::delete()
        .from_table(table_of(target, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(unkept_keys()))
        .take();
    let retired = Query::update()
        .table(table_of(target, Entities::Table))
        .values([
            (Entities::WriteCount, next_write_count(Entities::Table)),
            (Entities::CreatedAt, Keyword::Null.into()),
            (Entities::ModifiedAt, Keyword::Null.into()),
            (Entities::ExpiresAt, Keyword::Null.into()),
            (Entities::MinAttributeExpiresAt, Keyword::Null.into()),
            (
                Entities::DeletedOrQueuedAt,
                Func::greatest([Expr::col((Entities::Table, Entities::DeletedOrQueuedAt)), Expr::val(accepted_at)]).into(),
            ),
        ])
        .and_where(Expr::col(Entities::EntityKey).in_subquery(kept_keys()))
        .take();
    let retired_attributes = Query::delete()
        .from_table(table_of(target, CurrentAttributes::Table))
        .and_where(Expr::col(CurrentAttributes::EntityKey).in_subquery(kept_keys()))
        .take();
    let schema = schema_iden(target);
    let memberships = side_departure(
        Memberships {
            side: MembershipSide::Current,
            schema: schema.as_ref(),
        },
        kept_keys(),
    );

    let mut with = WithClause::new();
    with.cte(CommonTableExpression::new().table_name(Doomed).query(doomed).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(Srch)
                .columns([
                    EntitySearch::EntityKey.into_iden(),
                    SearchImage::OldTypes.into_iden(),
                    SearchImage::OldInstanceNames.into_iden(),
                    SearchImage::OldInstanceKinds.into_iden(),
                ])
                .query(search)
                .to_owned(),
        )
        .cte(CommonTableExpression::new().table_name(Gone).query(gone).to_owned())
        .cte(CommonTableExpression::new().table_name(Retired).query(retired).to_owned())
        .cte(CommonTableExpression::new().table_name(RetiredAttributes).query(retired_attributes).to_owned())
        .cte(CommonTableExpression::new().table_name(Unflagged).query(memberships.cleared).to_owned())
        .cte(CommonTableExpression::new().table_name(Strays).query(memberships.deleted).to_owned());
    for census in CensusStatements::of(target, Srch, CensusChange::Removed)?.ctes() {
        with.cte(census);
    }
    let statement = with.query(deletion_answer(refused()));

    Ok(with_staged_alias(UnnestAliased::new(statement), &columns))
}

/// The delete's candidates, `doomed`: each identity row `staging` names that
/// holds a current state and still stands as its expectation states, locked,
/// with the system attributes the notification needs, whether the delete
/// keeps the row, and the latest instant it holds.
///
/// The lock reads each row as the writer it waited on left it, so its latest
/// instant is the one the deletion is compared with, and an enqueue committed
/// while the statement waited keeps the row.
fn doomed_candidates(target: &ContextTarget, staging: FunctionCall) -> SelectStatement {
    Query::select()
        .expr(Expr::col((E, Entities::EntityKey)))
        .expr(Expr::col((E, Entities::Id)))
        .expr(Expr::col((E, Entities::CreatedAt)))
        .expr(Expr::col((E, Entities::ModifiedAt)))
        .expr(Expr::col((E, Entities::ExpiresAt)))
        .expr_as(holds_evolution(E).or(Expr::col((E, Entities::DeletedOrQueuedAt)).is_not_null()), Kept)
        .expr_as(latest_instant(E), Latest)
        .from_function(staging, Staged)
        .join_as(
            JoinType::Join,
            table_of(target, Entities::Table),
            E,
            Expr::col((E, Entities::Id)).equals((Staged, StagedColumn::Id)),
        )
        .and_where(holds_current_state(E))
        .cond_where(expectation(Staged, E))
        .lock_with_tables(LockType::Update, [E.into_iden()])
        .take()
}

/// What the delete answers for every candidate: its system attributes, the
/// transaction timestamp, and its latest instant where `refused` left it
/// standing.
fn deletion_answer(refused: SimpleExpr) -> SelectStatement {
    Query::select()
        .columns([
            Entities::Id.into_iden(),
            Entities::CreatedAt.into_iden(),
            Entities::ModifiedAt.into_iden(),
            Entities::ExpiresAt.into_iden(),
        ])
        .expr_as(now(), Alias::new(COMMITTED_AT))
        .expr_as(latest_where_refused(refused, Expr::col(Latest)), LatestInstant)
        .from(Doomed)
        .take()
}

impl EntityRepository {
    /// Delete every entity in `ids` whose expectation still holds and that
    /// holds no instant later than `accepted_at`, returning the system
    /// attributes each one held and the instant of the transaction that
    /// removed it, and every entity left standing for holding a later
    /// instant, with that instant.
    ///
    /// The search row is deleted beside the identity row so the discovery
    /// census can count out what it held; the membership and attribute
    /// instance rows of the current state go with it, and the entity's
    /// temporal evolution deliberately survives, keeping the identity row:
    /// clause 5.6.6 deletes the Entity, and only clause 5.6.16 deletes its
    /// history. An Entity whose history is queued keeps the row as well, for
    /// the Evolution the drain brings into being.
    ///
    /// `expected` is positionally aligned with `ids`; a `None` entry
    /// deletes unconditionally.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Invariant`] when the expectation slice is not
    /// the same length as `ids`, a returned identifier is no longer a valid URI
    /// or the statement cannot be built, and [`ContextStoreError::Database`]
    /// when the delete fails.
    pub async fn delete_batch_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        ids: &[Identifier],
        expected: &[Option<EntityVersion>],
        accepted_at: DateTime<Utc>,
    ) -> Result<WrittenEntities<RemovedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(WrittenEntities::none());
        }
        if expected.len() != ids.len() {
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Delete,
                expectations: expected.len(),
                rows: ids.len(),
            }
            .into());
        }

        let rows = entity_delete_statement(target, ids, expected, accepted_at)?.to_query().fetch_all(&mut **tx);
        let rows = observe_statement(ObservedStatement::EntityDelete, rows).await?;

        let mut written = WrittenEntities::none();
        for row in rows {
            let id = stored_uri(row.try_get("id")?)?;
            match row.try_get::<Option<DateTime<Utc>>, _>(LatestInstant.to_string().as_str())? {
                Some(latest) => written.predated.push(PredatedEntity {
                    id,
                    latest: SystemInstant::new(latest),
                }),
                None => written.landed.push(RemovedEntity {
                    entity: DeletedEntity {
                        id,
                        created_at: row.try_get("created_at")?,
                        modified_at: row.try_get("modified_at")?,
                        expires_at: row.try_get("expires_at")?,
                    },
                    committed_at: StoreInstant::new(row.try_get(COMMITTED_AT)?),
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
    use crate::{entity::write::delete::entity_delete_statement, statement_text::text_of};
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::{DateTime, Utc};

    /// The instant every rendered delete is stamped at.
    fn accepted_at() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().expect("a valid instant")
    }

    /// The rendered delete, unwrapped for assertion.
    fn entity_delete_sql(target: &ContextTarget) -> String {
        text_of(&entity_delete_statement(target, &[], &[], accepted_at()).expect("the fixed delete builds"))
    }

    #[test]
    fn the_delete_locks_its_candidates_and_honours_the_expectation() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(sql.contains("UNNEST($1::text[], $2::bigint[], $3::bigint[])"), "{sql}");
        assert!(
            sql.starts_with(r#"WITH "doomed" AS (SELECT "e"."entity_key", "e"."id", "e"."created_at", "e"."modified_at", "e"."expires_at", "#),
            "{sql}"
        );
        assert!(sql.contains("AS d(id, expected_entity_key, expected_write_count)"), "{sql}");
        assert!(
            sql.contains(r#"JOIN "entities" AS "e" ON "e"."id" = "d"."id" WHERE "e"."created_at" IS NOT NULL AND"#),
            "an Entity without a current state is no candidate: {sql}"
        );
        assert!(
            sql.contains(
                r#""d"."expected_entity_key" IS NULL OR ("e"."entity_key" = "d"."expected_entity_key" AND "e"."write_count" = "d"."expected_write_count")"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#"FOR UPDATE OF "e")"#), "{sql}");
    }

    #[test]
    fn the_delete_removes_identity_rows_by_key_and_returns_what_the_notification_needs() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(
            sql.contains(
                r#""gone" AS (DELETE FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed" WHERE (NOT "latest" > $5) AND (NOT "kept")))"#
            ),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"SELECT "id", "created_at", "modified_at", "expires_at", NOW() AS "committed_at", (CASE WHEN ("latest" > $11) THEN "latest" ELSE NULL END) AS "latest_instant" FROM "doomed""#),
            "{sql}"
        );
    }

    /// Clause 4.8: the tombstones are recorded at the deletion's instant, so
    /// a candidate holding a later instant, its current state's, its Temporal
    /// Evolution's, or an earlier deletion's or queued write's, is locked and
    /// left standing, and its row answers that instant.
    #[test]
    fn the_delete_leaves_a_candidate_holding_a_later_instant_standing() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#"GREATEST("e"."modified_at", "e"."evolution_modified_at", "e"."deleted_or_queued_at") AS "latest" FROM UNNEST("#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""srch" ("entity_key", "old_types", "old_instance_names", "old_instance_kinds") AS (DELETE FROM "entity_search" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed" WHERE NOT "latest" > $4)"#),
            "{sql}"
        );
        pretty_assertions::assert_eq!(sql.matches(r#"NOT "latest" > $"#).count(), 6, "every removal leaves it: {sql}");
    }

    /// The queue is read under the statement's snapshot, which a deletion
    /// committed while the statement waited on its row is not in, so the
    /// latest instant comes from the locked row alone.
    #[test]
    fn the_delete_reads_no_queue() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(!sql.contains("temporal_append_intents"), "{sql}");
    }

    /// Clause 5.6.6 leaves the Temporal Evolution standing: an Entity holding
    /// one keeps its identity row with the current state taken off, its
    /// attribute instances removed and its memberships kept for the
    /// Evolution alone. No history relation is touched. The deletion moves the
    /// row's write count, so a version read before it never matches the Entity
    /// clause 5.6.1 creates anew on the same row, and leaves its instant on the
    /// row, which no later write of the current state may precede (clause 4.8).
    #[test]
    fn an_entity_with_a_temporal_evolution_keeps_its_identity_row() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        let kept = |placeholder: u8| format!(r#"(SELECT "entity_key" FROM "doomed" WHERE (NOT "latest" > ${placeholder}) AND "kept")"#);
        assert!(
            sql.contains(&format!(
                r#""retired" AS (UPDATE "entities" SET "write_count" = "entities"."write_count" + 1, "created_at" = NULL, "modified_at" = NULL, "expires_at" = NULL, "min_attribute_expires_at" = NULL, "deleted_or_queued_at" = GREATEST("entities"."deleted_or_queued_at", $6) WHERE "entity_key" IN {})"#,
                kept(7)
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""retired_attributes" AS (DELETE FROM "current_attributes" WHERE "entity_key" IN {})"#,
                kept(8)
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""unflagged" AS (UPDATE "entity_type_members" SET "is_current_member" = FALSE WHERE "entity_key" IN {} AND "is_evolution_member")"#,
                kept(9)
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""strays" AS (DELETE FROM "entity_type_members" WHERE "entity_key" IN {} AND (NOT "is_evolution_member"))"#,
                kept(10)
            )),
            "{sql}"
        );
        for relation in ["attribute_history", "temporal_attribute_streams", "scope_history"] {
            assert!(!sql.contains(relation), "entity deletion must not name {relation}: {sql}");
        }
    }

    /// An Entity whose history may still wait in the queue keeps its identity
    /// row without a Temporal Evolution, so the Evolution the drain brings
    /// into being and a later create land on it, the create held after the
    /// deletion (clause 4.8). The flag is read from the locked row, which an
    /// enqueue committed while the statement waited has raised.
    #[test]
    fn an_entity_with_queued_history_keeps_its_identity_row() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#""e"."expires_at", "e"."evolution_created_at" IS NOT NULL OR "e"."deleted_or_queued_at" IS NOT NULL AS "kept", GREATEST("#),
            "{sql}"
        );
    }

    /// The census counts out the search rows the delete removed, read from its
    /// own `RETURNING`.
    #[test]
    fn the_delete_counts_the_removed_search_rows_out_of_the_census() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#""srch" ("entity_key", "old_types", "old_instance_names", "old_instance_kinds") AS (DELETE FROM "entity_search" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed" WHERE NOT "latest" > $4) RETURNING "entity_key", "entity_type_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds")"#),
            "{sql}"
        );
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
        assert!(sql.contains("-1 AS \"sign\""), "{sql}");
    }
}
