//! Drawing revisions from the tenant's `context_source_change_clock`.
//!
//! Every write of a context source registration and every client write of a
//! context source subscription advances the clock inside its own transaction.
//! The advance takes the singleton row's lock, held until the transaction
//! ends, so revisions are drawn in the order the writes commit, across every
//! broker sharing the database. Each writer advances the clock as its last
//! lock-taking statement: a writer holding the clock never waits on a lock
//! another writer takes before advancing it, so the clock adds no deadlock.
//!
//! A writer waiting on the row lock holds a connection of the tenant pool, so
//! every writer begins its transaction through the tenant's clock-writer share
//! of the pool, which admits no more writers than the write reserve keeps
//! connections. However many writers queue on the row, the rest of the pool
//! stays free for every other request. The share orders nothing. The row lock
//! still draws the revisions in commit order.

use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_postgresql_pool::pool_share::SharedTransaction;
use andromeda_postgresql_schema::context_source_change_clock::ContextSourceChangeClock;
use andromeda_postgresql_seaquery::{literal::int_literal, statement::Statement};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, Query, QueryStatementBuilder, SimpleExpr, UpdateStatement, WithClause};

/// The common table expression one statement advances the clock in.
#[derive(Iden, Clone, Copy)]
#[iden = "advanced_clock"]
struct AdvancedClock;

/// Advance the clock by one, returning the revision drawn.
fn advance_statement() -> UpdateStatement {
    Query::update()
        .table(ContextSourceChangeClock::Table)
        .value(
            ContextSourceChangeClock::Revision,
            Expr::col(ContextSourceChangeClock::Revision).add(int_literal(1)),
        )
        .returning_col(ContextSourceChangeClock::Revision)
        .to_owned()
}

/// Advance the clock inside `transaction`, begun through the tenant's
/// clock-writer share, returning the revision drawn.
///
/// # Errors
///
/// Returns the driver error when the statement fails.
pub(crate) async fn advance(transaction: &mut SharedTransaction) -> Result<CsourceChangeRevision, sqlx::Error> {
    let (revision,): (i64,) = advance_statement().to_query_as().fetch_one(&mut **transaction).await?;
    Ok(CsourceChangeRevision::from(revision))
}

/// The `WITH` clause a single statement advances the clock in, the revision
/// drawn read through [`advanced_revision`].
pub(crate) fn advance_clause() -> WithClause {
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(AdvancedClock).query(advance_statement()).to_owned())
        .to_owned()
}

/// The revision [`advance_clause`] drew, as a scalar subquery.
pub(crate) fn advanced_revision() -> SimpleExpr {
    let drawn = Query::select().column(ContextSourceChangeClock::Revision).from(AdvancedClock).to_owned();
    SimpleExpr::SubQuery(None, Box::new(drawn.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::context_source_change_clock::{advance_clause, advance_statement, advanced_revision};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn an_advance_increments_the_singleton_and_returns_the_revision_drawn() {
        let (sql, _) = advance_statement().build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"UPDATE "context_source_change_clock" SET "revision" = "revision" + (1) RETURNING "revision""#
        );
    }

    #[test]
    fn a_statement_reads_the_revision_its_own_advance_drew() {
        let sql = Query::select()
            .expr(advanced_revision())
            .take()
            .with(advance_clause())
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"WITH "advanced_clock" AS (UPDATE "context_source_change_clock" SET "revision" = "revision" + (1) RETURNING "revision") SELECT (SELECT "revision" FROM "advanced_clock")"#
        );
    }
}
