//! The 64-bit transaction identifiers PostgreSQL stamps writes and snapshots
//! with.
//!
//! An `xid8` never wraps around, so ordering two of them is ordering the
//! transactions that took them: a transaction that finished before a snapshot
//! was taken holds an identifier below that snapshot's `xmin`, and every
//! transaction still running when it was taken, or started after, holds one at
//! or above it. Every transaction whose writes a snapshot sees holds an
//! identifier below that snapshot's `xmax`, the first one not yet assigned
//! when it was taken.

use crate::{cast::cast, pg_type::PostgresType};
use sea_query::{Func, Iden, SimpleExpr};

/// The `pg_current_xact_id()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_current_xact_id"]
struct PgCurrentXactId;

/// The `pg_current_snapshot()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_current_snapshot"]
struct PgCurrentSnapshot;

/// The `pg_snapshot_xmin()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_snapshot_xmin"]
struct PgSnapshotXmin;

/// The `pg_snapshot_xmax()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_snapshot_xmax"]
struct PgSnapshotXmax;

/// PostgreSQL: `pg_current_xact_id()` - the `xid8` of the current transaction,
/// assigning one if it has none yet.
#[must_use]
pub fn current_transaction_id() -> SimpleExpr {
    Func::cust(PgCurrentXactId).into()
}

/// PostgreSQL: `pg_snapshot_xmin(pg_current_snapshot())::text::bigint` - the
/// oldest transaction still running when the current statement's snapshot was
/// taken, as a `bigint`.
///
/// Read in the same statement as the rows it qualifies, it is the horizon below
/// which every write the statement could see has already committed or aborted.
/// `xid8` has no direct cast to `bigint`, so it passes through its text form;
/// the epoch-extended value stays far inside the signed range.
#[must_use]
pub fn snapshot_xmin() -> SimpleExpr {
    let xmin: SimpleExpr = Func::cust(PgSnapshotXmin).arg(Func::cust(PgCurrentSnapshot)).into();
    cast(cast(xmin, PostgresType::Text), PostgresType::Bigint)
}

/// PostgreSQL: `pg_snapshot_xmax(pg_current_snapshot())` - the first `xid8`
/// not yet assigned when the current statement's snapshot was taken.
///
/// Every row the statement sees was written by a transaction below it, so a
/// visible row whose stored transaction identifier is at or above it took that
/// identifier from another cluster's counter, as a dump restored into a new
/// cluster carries it.
#[must_use]
pub fn snapshot_xmax() -> SimpleExpr {
    Func::cust(PgSnapshotXmax).arg(Func::cust(PgCurrentSnapshot)).into()
}

/// A `bigint` reading of an `xid8`, such as [`snapshot_xmin`] produced, cast
/// back to `xid8` so it compares against a stored transaction identifier.
#[must_use]
pub fn as_transaction_id(position: SimpleExpr) -> SimpleExpr {
    cast(cast(position, PostgresType::Text), PostgresType::Xid8)
}

#[cfg(test)]
mod tests {
    use crate::transaction_id::{as_transaction_id, current_transaction_id, snapshot_xmax, snapshot_xmin};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_current_transaction_id_renders_the_xid8_function() {
        let sql = Query::select().expr(current_transaction_id()).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_current_xact_id()");
    }

    #[test]
    fn the_snapshot_horizon_reads_the_statement_snapshot_as_a_bigint() {
        let sql = Query::select().expr(snapshot_xmin()).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_snapshot_xmin(pg_current_snapshot())::text::bigint");
    }

    #[test]
    fn the_snapshot_ceiling_reads_the_statement_snapshot_as_a_transaction_id() {
        let sql = Query::select().expr(snapshot_xmax()).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_snapshot_xmax(pg_current_snapshot())");
    }

    #[test]
    fn a_bigint_position_is_cast_back_to_a_transaction_id() {
        let sql = Query::select()
            .expr(as_transaction_id(Expr::val(7_i64)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT 7::text::xid8");
    }
}
