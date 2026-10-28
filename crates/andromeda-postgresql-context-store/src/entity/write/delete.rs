//! The batch delete: one statement that removes each staged entity whose
//! expectation still holds and hands back the system attributes it held.

use crate::{
    entity::{
        census::delta::{CensusChange, CensusStatements},
        mapper::stored_uri,
        repo::EntityRepository,
        write::{
            search_image::SearchImage,
            staging::{
                Doomed,
                Srch,
                Staged,
                StagedColumn,
                delete_staging_columns,
                expectation,
                expectation_arrays,
                next_write_count,
                staging_source,
                with_staged_alias,
            },
        },
    },
    error::ContextStoreError,
    store_invariant::{GuardedEntityWrite, StoreInvariant},
};
use andromeda_context_commit::{deleted_entity::DeletedEntity, entity_version::EntityVersion};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
use andromeda_postgresql_seaquery::{pg_array::text_array, statement::Statement, unnest::UnnestAliased};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, IntoIden, JoinType, Keyword, LockType, Query, SimpleExpr, WithClause, WithQuery};
use sqlx::{Postgres, Row, Transaction};

/// The statement name the entity set-delete is observed under.
const ENTITY_DELETE: StatementName = StatementName("entity-delete");

/// The delete CTE, `gone`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
struct Gone;

/// Whether a doomed Entity keeps its identity row for its Temporal Evolution,
/// `kept`, a column of `doomed`.
#[derive(Iden, Clone, Copy)]
#[iden = "kept"]
struct Kept;

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

/// Build the batch delete of `ids`, each held to its entry of `expected`.
///
/// ```sql
/// WITH doomed AS (SELECT e.entity_key, e.id, e.created_at, e.modified_at, e.expires_at,
///                        e.evolution_created_at IS NOT NULL AS kept
///                 FROM UNNEST(…) AS d(id, expected_entity_key, expected_write_count)
///                 JOIN entities e ON e.id = d.id
///                 WHERE e.created_at IS NOT NULL
///                   AND (d.expected_entity_key IS NULL
///                        OR (e.entity_key = d.expected_entity_key AND e.write_count = d.expected_write_count))
///                 FOR UPDATE OF e),
///      srch(entity_key, old_types, old_instance_names, old_instance_kinds) AS
///              (DELETE FROM entity_search WHERE entity_key IN (SELECT entity_key FROM doomed)
///               RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds),
///      gone AS (DELETE FROM entities WHERE entity_key IN (SELECT entity_key FROM doomed WHERE NOT kept)),
///      retired AS (UPDATE entities SET write_count = entities.write_count + 1, created_at = NULL, modified_at = NULL,
///                  expires_at = NULL, min_attribute_expires_at = NULL WHERE entity_key IN (SELECT entity_key FROM doomed WHERE kept)),
///      retired_attributes AS (DELETE FROM current_attributes WHERE entity_key IN (… WHERE kept)),
///      unflagged AS (UPDATE entity_type_members SET is_current_member = FALSE WHERE entity_key IN (… WHERE kept) AND is_evolution_member),
///      strays AS (DELETE FROM entity_type_members WHERE entity_key IN (… WHERE kept) AND NOT is_evolution_member),
///      census_types AS (INSERT INTO entity_type_census … FROM srch …), census_type_attributes AS (…), census_attributes AS (…)
/// SELECT id, created_at, modified_at, expires_at FROM doomed
/// ```
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
/// The search row is deleted by name so that the rows it held come back: the
/// discovery census counts every one of their types and attribute instances
/// out, from the row the delete removed rather than from the statement's
/// snapshot of it. Clause 5.6.6 leaves the Temporal Evolution standing, so no
/// history relation is touched, and an Entity holding one keeps its identity
/// row with its current state taken off: its attribute instances go, and its
/// type memberships keep the Evolution's flag alone. Everything of an Entity
/// without one goes with the identity row: the type memberships through
/// their cascading foreign key, the attribute instances through the
/// statement-level trigger on `entities`, which removes the instances of
/// every deleted entity in one pass. An Entity without a current state is not
/// a candidate at all, so a core delete of it finds nothing. The system
/// attributes come back because the clause 5.6.10 notification carries them
/// and cannot read them once the rows are gone; the attributes do not, because
/// every caller that needs them read them before the delete.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if a census ledger insert ever stops
/// matching its column list, which the fixed shapes make unreachable.
pub(super) fn entity_delete_statement(
    target: &ContextTarget,
    ids: &[Identifier],
    expected: &[Option<EntityVersion>],
) -> Result<UnnestAliased<WithQuery>, ContextStoreError> {
    let columns = delete_staging_columns();
    let ids = ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
    let mut arrays = vec![text_array(&ids)];
    arrays.extend(expectation_arrays(expected));

    let mut doomed = Query::select();
    doomed
        .expr(Expr::col((E, Entities::EntityKey)))
        .expr(Expr::col((E, Entities::Id)))
        .expr(Expr::col((E, Entities::CreatedAt)))
        .expr(Expr::col((E, Entities::ModifiedAt)))
        .expr(Expr::col((E, Entities::ExpiresAt)))
        .expr_as(holds_evolution(E), Kept)
        .from_function(staging_source(&columns, arrays), Staged)
        .join_as(
            JoinType::Join,
            table_of(target, Entities::Table),
            E,
            Expr::col((E, Entities::Id)).equals((Staged, StagedColumn::Id)),
        )
        .and_where(holds_current_state(E))
        .cond_where(expectation(Staged, E))
        .lock_with_tables(LockType::Update, [E.into_iden()]);

    let doomed_keys = || Query::select().column(Entities::EntityKey).from(Doomed).take();
    let doomed_keys_where = |kept: SimpleExpr| Query::select().column(Entities::EntityKey).from(Doomed).and_where(kept).take();
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
    with.cte(CommonTableExpression::new().table_name(Doomed).query(doomed.take()).to_owned())
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
    let statement = with.query(
        Query::select()
            .columns([
                Entities::Id.into_iden(),
                Entities::CreatedAt.into_iden(),
                Entities::ModifiedAt.into_iden(),
                Entities::ExpiresAt.into_iden(),
            ])
            .from(Doomed)
            .take(),
    );

    Ok(with_staged_alias(UnnestAliased::new(statement), &columns))
}

impl EntityRepository {
    /// Delete every entity in `ids` whose expectation still holds, returning
    /// the system attributes each one held.
    ///
    /// The search row is deleted beside the identity row so the discovery
    /// census can count out what it held; the membership and attribute
    /// instance rows of the current state go with it, and the entity's
    /// temporal evolution deliberately survives, keeping the identity row:
    /// clause 5.6.6 deletes the Entity, and only clause 5.6.16 deletes its
    /// history.
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
    ) -> Result<Vec<DeletedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if expected.len() != ids.len() {
            return Err(StoreInvariant::ExpectationCount {
                write: GuardedEntityWrite::Delete,
                expectations: expected.len(),
                rows: ids.len(),
            }
            .into());
        }

        let rows = entity_delete_statement(target, ids, expected)?.to_query().fetch_all(&mut **tx);
        let rows = observe_statement(ENTITY_DELETE, rows).await?;

        rows.into_iter()
            .map(|row| {
                Ok(DeletedEntity {
                    id: stored_uri(row.try_get("id")?)?,
                    created_at: row.try_get("created_at")?,
                    modified_at: row.try_get("modified_at")?,
                    expires_at: row.try_get("expires_at")?,
                })
            })
            .collect()
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

    /// The rendered delete, unwrapped for assertion.
    fn entity_delete_sql(target: &ContextTarget) -> String {
        text_of(&entity_delete_statement(target, &[], &[]).expect("the fixed delete builds"))
    }

    #[test]
    fn the_delete_locks_its_candidates_and_honours_the_expectation() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(sql.contains("UNNEST($1::text[], $2::bigint[], $3::bigint[])"), "{sql}");
        assert!(
            sql.starts_with(r#"WITH "doomed" AS (SELECT "e"."entity_key", "e"."id", "e"."created_at", "e"."modified_at", "e"."expires_at", "e"."evolution_created_at" IS NOT NULL AS "kept" FROM UNNEST("#),
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
            sql.contains(r#""gone" AS (DELETE FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed" WHERE NOT "kept"))"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"SELECT "id", "created_at", "modified_at", "expires_at" FROM "doomed""#),
            "{sql}"
        );
    }

    /// Clause 5.6.6 leaves the Temporal Evolution standing: an Entity holding
    /// one keeps its identity row with the current state taken off, its
    /// attribute instances removed and its memberships kept for the
    /// Evolution alone. No history relation is touched. The deletion moves the
    /// row's write count, so a version read before it never matches the Entity
    /// clause 5.6.1 creates anew on the same row.
    #[test]
    fn an_entity_with_a_temporal_evolution_keeps_its_identity_row() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        let kept = r#"(SELECT "entity_key" FROM "doomed" WHERE "kept")"#;
        assert!(
            sql.contains(&format!(
                r#""retired" AS (UPDATE "entities" SET "write_count" = "entities"."write_count" + 1, "created_at" = NULL, "modified_at" = NULL, "expires_at" = NULL, "min_attribute_expires_at" = NULL WHERE "entity_key" IN {kept})"#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""retired_attributes" AS (DELETE FROM "current_attributes" WHERE "entity_key" IN {kept})"#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""unflagged" AS (UPDATE "entity_type_members" SET "is_current_member" = FALSE WHERE "entity_key" IN {kept} AND "is_evolution_member")"#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(
                r#""strays" AS (DELETE FROM "entity_type_members" WHERE "entity_key" IN {kept} AND (NOT "is_evolution_member"))"#
            )),
            "{sql}"
        );
        for relation in ["attribute_history", "temporal_attribute_streams", "scope_history"] {
            assert!(!sql.contains(relation), "entity deletion must not name {relation}: {sql}");
        }
    }

    /// The census counts out the search rows the delete removed, read from its
    /// own `RETURNING`.
    #[test]
    fn the_delete_counts_the_removed_search_rows_out_of_the_census() {
        let sql = entity_delete_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#""srch" ("entity_key", "old_types", "old_instance_names", "old_instance_kinds") AS (DELETE FROM "entity_search" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed") RETURNING "entity_key", "entity_type_name_keys", "attribute_instance_name_keys", "attribute_instance_kinds")"#),
            "{sql}"
        );
        for ledger in ["entity_type_census", "entity_type_attribute_census", "attribute_census"] {
            assert!(sql.contains(&format!(r#"INSERT INTO "{ledger}""#)), "{ledger}: {sql}");
        }
        assert!(sql.contains("-1 AS \"sign\""), "{sql}");
    }
}
