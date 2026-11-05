//! Giving up the Temporal Evolution of identity rows.
//!
//! One `entities` row serves an Entity's current state and its Temporal
//! Evolution. A Temporal Evolution deletion (clause 5.6.16), and retention once
//! an Evolution's history has left storage, give the Evolution up: an Entity
//! without a current state loses its identity row, whose streams, instance
//! claims and type memberships cascade from it; an Entity with one keeps the
//! row with the Evolution's columns cleared, its instance claims released and
//! its type memberships kept for the current state alone. The caller removes
//! the history and the streams first, and holds the identity rows' locks.
//!
//! A kept row's `deleted_or_queued_at` is cleared unless the Entity's history
//! still waits in the queue. Its current state's `modified_at` already
//! follows every deletion before it, and the history the drained writes left
//! is gone, so a later deletion of an Entity with neither an Evolution nor
//! queued history removes its row. The caller took the locks in a statement
//! of its own, so this statement's snapshot holds every intent the Entity's
//! writers committed, and no writer queues more before the caller commits.

use andromeda_postgresql_query::{entity_sides::holds_evolution, type_membership::Memberships, type_membership_moves::side_departure};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal::AttributeHistoryInstanceClaims, type_membership::MembershipSide};
use andromeda_postgresql_seaquery::{aggregate::count_all, branch::qualified};
use sea_query::{CaseStatement, CommonTableExpression, DynIden, Expr, ExprTrait, Iden, Keyword, Query, SelectStatement, SimpleExpr, WithClause, WithQuery};

/// The identity rows whose Evolution the statement gives up, `vacated`.
#[derive(Iden, Clone, Copy)]
#[iden = "vacated"]
struct Vacated;

/// Whether a vacated row keeps a current state, `current`, a column of
/// `vacated`.
#[derive(Iden, Clone, Copy)]
#[iden = "current"]
struct Current;

/// The CTE deleting the identity rows left with neither side, `gone`.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
struct Gone;

/// The CTE clearing the Evolution's columns of the rows a current state
/// keeps, `retired`.
#[derive(Iden, Clone, Copy)]
#[iden = "retired"]
struct Retired;

/// The CTE releasing the instance claims of the rows a current state keeps,
/// `released_claims`.
#[derive(Iden, Clone, Copy)]
#[iden = "released_claims"]
struct ReleasedClaims;

/// The CTE clearing the Evolution's flag of the memberships a current state
/// keeps, `unflagged`.
#[derive(Iden, Clone, Copy)]
#[iden = "unflagged"]
struct Unflagged;

/// The CTE deleting the memberships the current state does not hold,
/// `strays`.
#[derive(Iden, Clone, Copy)]
#[iden = "strays"]
struct Strays;

/// Give up the Temporal Evolution of every identity row of the branch
/// `schema` names whose key `keys` selects and which holds one, answering how
/// many it gave up.
#[must_use]
pub(crate) fn evolution_vacancy_statement(schema: Option<&DynIden>, keys: SelectStatement) -> WithQuery {
    let vacated = Query::select()
        .column(Entities::EntityKey)
        .expr_as(Expr::col(Entities::CreatedAt).is_not_null(), Current)
        .from(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(keys))
        .and_where(holds_evolution(Entities::Table))
        .take();
    let vacated_where = |current: SimpleExpr| Query::select().column(Entities::EntityKey).from(Vacated).and_where(current).take();
    let kept = || vacated_where(Expr::col(Current));
    let unkept = || vacated_where(Expr::col(Current).not());
    let gone = Query::delete()
        .from_table(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).in_subquery(unkept()))
        .take();
    let retired = Query::update()
        .table(qualified(schema, Entities::Table))
        .values([
            (Entities::EvolutionCreatedAt, Keyword::Null.into()),
            (Entities::EvolutionModifiedAt, Keyword::Null.into()),
            (Entities::EvolutionActiveUntil, Keyword::Null.into()),
            (Entities::EvolutionTypeNameKeys, Keyword::Null.into()),
            (Entities::DeletedOrQueuedAt, queued_instant_kept(schema)),
        ])
        .and_where(Expr::col(Entities::EntityKey).in_subquery(kept()))
        .take();
    let released_claims = Query::delete()
        .from_table(qualified(schema, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::col(AttributeHistoryInstanceClaims::EntityKey).in_subquery(kept()))
        .take();
    let memberships = side_departure(
        Memberships {
            side: MembershipSide::Evolution,
            schema,
        },
        kept(),
    );
    let mut with = WithClause::new();
    with.cte(CommonTableExpression::new().table_name(Vacated).query(vacated).to_owned())
        .cte(CommonTableExpression::new().table_name(Gone).query(gone).to_owned())
        .cte(CommonTableExpression::new().table_name(Retired).query(retired).to_owned())
        .cte(CommonTableExpression::new().table_name(ReleasedClaims).query(released_claims).to_owned())
        .cte(CommonTableExpression::new().table_name(Unflagged).query(memberships.cleared).to_owned())
        .cte(CommonTableExpression::new().table_name(Strays).query(memberships.deleted).to_owned());
    with.query(Query::select().expr(count_all()).from(Vacated).take())
}

/// The `deleted_or_queued_at` a row its current state keeps is left with: as
/// it was while an intent of the Entity is queued, NULL otherwise. A Snapshot
/// branch records its history in the write's own transaction, so nothing of
/// it is queued.
fn queued_instant_kept(schema: Option<&DynIden>) -> SimpleExpr {
    if schema.is_some() {
        return Keyword::Null.into();
    }
    let queued = Query::select()
        .expr(Expr::val(1))
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col((TemporalAppendIntents::Table, TemporalAppendIntents::EntityId)).equals((Entities::Table, Entities::Id)))
        .take();
    CaseStatement::new()
        .case(Expr::exists(queued), Expr::col((Entities::Table, Entities::DeletedOrQueuedAt)))
        .finally(Keyword::Null)
        .into()
}

#[cfg(test)]
mod tests {
    use crate::evolution_vacancy::evolution_vacancy_statement;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query, SelectStatement};

    fn keys() -> SelectStatement {
        Query::select().expr(Expr::val(7)).take()
    }

    fn rendered() -> String {
        evolution_vacancy_statement(None, keys()).to_string(PostgresQueryBuilder)
    }

    /// Only an identity row holding a Temporal Evolution is vacated, and
    /// whether it keeps a current state decides what happens to it.
    #[test]
    fn the_vacated_rows_hold_an_evolution_and_say_whether_a_current_state_keeps_them() {
        assert!(
            rendered().starts_with(concat!(
                r#"WITH "vacated" AS (SELECT "entity_key", "created_at" IS NOT NULL AS "current" FROM "entities" "#,
                r#"WHERE "entity_key" IN (SELECT 7) AND "entities"."evolution_created_at" IS NOT NULL)"#
            )),
            "{}",
            rendered()
        );
        assert!(rendered().ends_with(r#"SELECT COUNT(*) FROM "vacated""#), "{}", rendered());
    }

    /// Clause 5.6.16 removes the Temporal Evolution alone: an Entity with a
    /// current state keeps its identity row, cleared of the Evolution, and an
    /// Entity without one loses it.
    #[test]
    fn a_current_state_keeps_the_identity_row_and_its_absence_deletes_it() {
        let sql = rendered();
        assert!(
            sql.contains(r#""gone" AS (DELETE FROM "entities" WHERE "entity_key" IN (SELECT "entity_key" FROM "vacated" WHERE NOT "current"))"#),
            "{sql}"
        );
        assert!(
            sql.contains(concat!(
                r#""retired" AS (UPDATE "entities" SET "evolution_created_at" = NULL, "evolution_modified_at" = NULL, "evolution_active_until" = NULL, "evolution_type_name_keys" = NULL, "#,
                r#""deleted_or_queued_at" = "#
            )),
            "{sql}"
        );
    }

    /// A kept row keeps its queued instant only while the Entity's history
    /// still waits in the queue, so a later deletion of an Entity left with
    /// neither an Evolution nor queued history removes its row.
    #[test]
    fn a_kept_row_keeps_its_queued_instant_only_while_its_history_is_queued() {
        let sql = rendered();
        assert!(
            sql.contains(concat!(
                r#""deleted_or_queued_at" = (CASE WHEN (EXISTS(SELECT 1 FROM "temporal_append_intents" WHERE "temporal_append_intents"."entity_id" = "entities"."id")) "#,
                r#"THEN "entities"."deleted_or_queued_at" ELSE NULL END) WHERE "entity_key" IN (SELECT "entity_key" FROM "vacated" WHERE "current"))"#
            )),
            "{sql}"
        );
    }

    /// The instance claims and the Evolution's memberships of a kept row go
    /// with the Evolution; those of a deleted row cascade from it.
    #[test]
    fn a_kept_row_releases_its_claims_and_its_evolution_memberships() {
        let sql = rendered();
        let kept = r#"(SELECT "entity_key" FROM "vacated" WHERE "current")"#;
        for expected in [
            format!(r#""released_claims" AS (DELETE FROM "attribute_history_instance_claims" WHERE "entity_key" IN {kept})"#),
            format!(r#""unflagged" AS (UPDATE "entity_type_members" SET "is_evolution_member" = FALSE WHERE "entity_key" IN {kept} AND "is_current_member")"#),
            format!(r#""strays" AS (DELETE FROM "entity_type_members" WHERE "entity_key" IN {kept} AND (NOT "is_current_member"))"#),
        ] {
            assert!(sql.contains(&expected), "{expected}: {sql}");
        }
    }

    #[test]
    fn a_snapshot_branch_vacates_its_own_rows() {
        let schema = Alias::new("snap_3").into_iden();
        let sql = evolution_vacancy_statement(Some(&schema), keys()).to_string(PostgresQueryBuilder);
        for relation in ["entities", "attribute_history_instance_claims", "entity_type_members"] {
            assert!(sql.contains(&format!(r#""snap_3"."{relation}""#)), "{relation}: {sql}");
        }
        assert_eq!(sql.matches(r#"FROM "entities""#).count(), 0, "{sql}");
        assert!(!sql.contains("temporal_append_intents"), "a Snapshot branch queues nothing: {sql}");
        assert!(
            sql.contains(r#""evolution_type_name_keys" = NULL, "deleted_or_queued_at" = NULL WHERE"#),
            "{sql}"
        );
    }
}
