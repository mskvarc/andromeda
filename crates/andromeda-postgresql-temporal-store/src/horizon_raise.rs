//! Raising the activity horizon of Temporal Evolutions, `evolution_active_until`,
//! to cover the instants a write records.
//!
//! Each statement that writes an instant into `attribute_history` raises the
//! horizon to cover that instant, and nothing lowers it while the Evolution
//! lasts. A horizon left above the instants an Evolution still holds (after an
//! instance deletion, or a retention sweep that keeps the Evolution) only adds
//! a candidate the window match then rejects. The horizon rounds to the end of
//! an hour, so a raise moves it only when a write passes into a later hour,
//! about once an hour for an Entity written steadily, and every other write
//! leaves the indexed column alone and stays a heap-only update.

use andromeda_postgresql_schema::current::Entities;
use sea_query::{Condition, Expr, ExprTrait, Iden, IntoIden, IntoTableRef, Query, SelectStatement, SimpleExpr, TableRef, UpdateStatement};

/// The horizon each Evolution of a raise reaches, `reached`, both the
/// relation and its horizon column.
#[derive(Iden, Clone, Copy)]
#[iden = "reached"]
struct Reached;

/// Raise the horizon of every Evolution `reached` names to the one it reaches
/// there, in the identity rows of `entities`.
///
/// `reached` yields `entity_key` and the horizon its rows reach, one row per
/// Evolution. A row whose horizon is already as late is left alone, so a write
/// inside the current hour updates no identity row, and a NULL horizon raises
/// nothing.
#[must_use]
pub(crate) fn horizon_raise_statement(entities: impl IntoTableRef, reached: SelectStatement) -> UpdateStatement {
    let stored = || Expr::col((Entities::Table, Entities::EvolutionActiveUntil));
    let reached_horizon = || Expr::col((Reached, Reached));
    Query::update()
        .table(entities)
        .value(Entities::EvolutionActiveUntil, reached_horizon())
        .from(TableRef::SubQuery(Box::new(reached), Reached.into_iden()))
        .and_where(Expr::col((Entities::Table, Entities::EntityKey)).equals((Reached, Entities::EntityKey)))
        .cond_where(Condition::any().add(stored().is_null()).add(stored().lt(reached_horizon())))
        .to_owned()
}

/// `select` projecting `horizon` under the name [`horizon_raise_statement`]
/// reads it by.
pub(crate) fn project_horizon(select: &mut SelectStatement, horizon: SimpleExpr) {
    select.expr_as(horizon, Reached);
}

#[cfg(test)]
mod tests {
    use crate::horizon_raise::{horizon_raise_statement, project_horizon};
    use andromeda_postgresql_query::evolution_horizon::horizon_of;
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_raise_moves_only_a_horizon_that_is_missing_or_earlier() {
        let mut reached = Query::select();
        reached.column(Entities::EntityKey).from(Alias::new("written"));
        project_horizon(&mut reached, horizon_of(Expr::col(Alias::new("observed_at"))));
        let sql = horizon_raise_statement(Entities::Table, reached).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"UPDATE "entities" SET "evolution_active_until" = "reached"."reached""#,
                r#" FROM (SELECT "entity_key", date_trunc('hour', "observed_at", 'UTC') + (make_interval(secs => 3600)) AS "reached" FROM "written") AS "reached""#,
                r#" WHERE "entities"."entity_key" = "reached"."entity_key""#,
                r#" AND ("entities"."evolution_active_until" IS NULL OR "entities"."evolution_active_until" < "reached"."reached")"#
            )
        );
    }
}
