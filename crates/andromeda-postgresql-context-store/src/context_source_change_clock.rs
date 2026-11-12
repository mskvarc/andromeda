//! Drawing revisions from the tenant's `context_source_change_clock`.
//!
//! Every write of a context source registration and every client write of a
//! context source subscription advances the clock inside its own transaction.
//! The advance takes the singleton row's lock, held until the transaction
//! ends, so revisions are drawn in the order the writes commit, across every
//! broker sharing the database. Each writer advances the clock in its last
//! statement, sent in one message with the transaction's `COMMIT`, so the row
//! is held only while the server runs that statement and the commit, never
//! across a round trip to the broker. A writer holding the clock therefore
//! never waits on a lock another writer takes before advancing it, so the clock
//! adds no deadlock.
//!
//! A writer waiting on the row lock holds a connection of the tenant pool, so
//! every writer begins its transaction through the tenant's clock-writer share
//! of the pool, which admits no more writers than the write reserve keeps
//! connections. However many writers queue on the row, the rest of the pool
//! stays free for every other request. The share orders nothing. The row lock
//! still draws the revisions in commit order.

use crate::store_fault::persistence_error;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_pool::pool_share::SharedTransaction;
use andromeda_postgresql_schema::context_source_change_clock::ContextSourceChangeClock;
use andromeda_postgresql_seaquery::{committing_statement::CommittingStatement, literal::int_literal};
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

/// Advance the clock as the last statement of `transaction`, begun through
/// the tenant's clock-writer share, and commit it in the same round trip,
/// returning the revision drawn.
///
/// # Errors
///
/// Returns the persistence failure of the port error `E` when the advance or
/// the commit fails. A failed commit returns no revision.
pub(crate) async fn advance_and_commit<E: From<PersistenceFailure>>(transaction: SharedTransaction) -> Result<CsourceChangeRevision, E> {
    let advance = CommittingStatement::new(&advance_statement()).map_err(|error| persistence_error::<E>(error))?;
    let revision: i64 = transaction.commit_fetching(&advance).await.map_err(|error| persistence_error::<E>(error))?;
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

/// The latest revision a committed write drew, as a scalar subquery read
/// without the row's lock.
pub(crate) fn committed_revision() -> SimpleExpr {
    let latest = Query::select()
        .column(ContextSourceChangeClock::Revision)
        .from(ContextSourceChangeClock::Table)
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(latest.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::context_source_change_clock::{advance_clause, advance_statement, advanced_revision, committed_revision};
    use andromeda_postgresql_seaquery::committing_statement::CommittingStatement;
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

    /// The commit is the advance's own round trip, so the advance is the
    /// whole statement and binds nothing the simple protocol cannot carry.
    #[test]
    fn the_advance_is_sent_with_the_commit_of_its_transaction() {
        assert!(CommittingStatement::new(&advance_statement()).is_ok());
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

    #[test]
    fn the_committed_revision_is_read_without_advancing_the_clock() {
        let sql = Query::select().expr(committed_revision()).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT (SELECT "revision" FROM "context_source_change_clock")"#);
    }
}
