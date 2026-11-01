//! The activity horizon of a Temporal Evolution, `evolution_active_until`.
//!
//! A bounded temporal window (clause 4.11) keeps an Evolution only for an
//! instance of a concerned attribute whose instant on the window's axis lies
//! inside it, so an Evolution none of whose instances reaches the window's
//! start on any axis can never match. The horizon bounds every such instant
//! from above: each statement that writes an instant into `attribute_history`
//! raises it to cover that instant, and nothing lowers it while the Evolution
//! lasts. A horizon left above the instants an Evolution still holds (after an
//! instance deletion, or a retention sweep that keeps the Evolution) only adds
//! a candidate the window match then rejects.
//!
//! The horizon is the end of the UTC hour holding the instant. A recording
//! write therefore changes it only when it moves the bound into a later hour,
//! about once an hour for an Entity written steadily, and every other write
//! leaves the indexed column alone and stays a heap-only update.

use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{date_field::date_trunc, interval::make_interval_secs};
use sea_query::{Condition, Expr, ExprTrait, Iden, IntoIden, IntoTableRef, Query, SelectStatement, SimpleExpr, TableRef, UpdateStatement};

/// The seconds of the hour a horizon rounds up to the end of.
const HOUR_SECONDS: f64 = 3_600.0;

/// The horizon each Evolution of a raise reaches, `reached`, both the
/// relation and its horizon column.
#[derive(Iden, Clone, Copy)]
#[iden = "reached"]
struct Reached;

/// The horizon covering `instant`: the end of the UTC hour holding it,
/// `date_trunc('hour', <instant>, 'UTC') + (make_interval(secs => 3600))`. An
/// instant on the hour gets the hour after it, which still bounds it.
#[must_use]
pub(crate) fn horizon_of(instant: SimpleExpr) -> SimpleExpr {
    date_trunc("hour", instant).add(make_interval_secs(Expr::val(HOUR_SECONDS)))
}

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
    use crate::evolution_horizon::{horizon_of, horizon_raise_statement, project_horizon};
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_horizon_is_the_end_of_the_utc_hour_holding_its_instant() {
        let sql = Query::select()
            .expr(horizon_of(Expr::col(Alias::new("observed_at"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT date_trunc('hour', "observed_at", 'UTC') + (make_interval(secs => 3600))"#);
    }

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
