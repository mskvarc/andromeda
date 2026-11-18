//! The 64-bit transaction identifiers PostgreSQL stamps writes and snapshots
//! with.
//!
//! An `xid8` never wraps around, so ordering two of them is ordering the
//! transactions that took them. A snapshot sees the writes of every transaction
//! below its `xmax`, the first identifier not yet assigned when it was taken,
//! except those still running then, which `pg_snapshot_xip` lists. A transaction
//! below `xmax` and not on that list had committed or aborted when the snapshot
//! was taken.

use crate::{cast::cast, pg_array::array_of_subquery, pg_type::PostgresType};
use sea_query::{Func, Iden, Query, SimpleExpr};

/// The `pg_current_xact_id()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_current_xact_id"]
struct PgCurrentXactId;

/// The `pg_current_snapshot()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_current_snapshot"]
struct PgCurrentSnapshot;

/// The `pg_snapshot_xmax()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_snapshot_xmax"]
struct PgSnapshotXmax;

/// The `pg_snapshot_xip()` function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_snapshot_xip"]
struct PgSnapshotXip;

/// PostgreSQL: `pg_current_xact_id()` - the `xid8` of the current transaction,
/// assigning one if it has none yet.
#[must_use]
pub fn current_transaction_id() -> SimpleExpr {
    Func::cust(PgCurrentXactId).into()
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

/// Every transaction still running when the current statement's snapshot was
/// taken, as the `bigint[]` that
/// `ARRAY (SELECT pg_snapshot_xip(pg_current_snapshot())::text::bigint)` reads.
///
/// Read in the same statement as the rows it qualifies, beside
/// [`snapshot_xmax`], it names exactly the transactions below `xmax` whose
/// writes the statement could not see.
#[must_use]
pub fn snapshot_in_progress_positions() -> SimpleExpr {
    let in_progress: SimpleExpr = Func::cust(PgSnapshotXip).arg(Func::cust(PgCurrentSnapshot)).into();
    array_of_subquery(Query::select().expr(as_position(in_progress)).take())
}

/// A `bigint` reading of an `xid8`.
///
/// `xid8` has no direct cast to `bigint`, so it passes through its text form;
/// the epoch-extended value stays far inside the signed range.
#[must_use]
pub fn as_position(transaction_id: SimpleExpr) -> SimpleExpr {
    cast(cast(transaction_id, PostgresType::Text), PostgresType::Bigint)
}

/// A `bigint` reading of an `xid8`, such as [`as_position`] produced, cast
/// back to `xid8` so it compares against a stored transaction identifier.
#[must_use]
pub fn as_transaction_id(position: SimpleExpr) -> SimpleExpr {
    cast(cast(position, PostgresType::Text), PostgresType::Xid8)
}

/// A `bigint[]` of [`as_position`] readings cast back to `xid8[]`, element by
/// element, so `= ANY` compares it against a stored transaction identifier
/// and an index on that identifier serves the comparison.
#[must_use]
pub fn as_transaction_ids(positions: SimpleExpr) -> SimpleExpr {
    cast(cast(positions, PostgresType::TextArray), PostgresType::Xid8Array)
}

#[cfg(test)]
mod tests {
    use crate::transaction_id::{as_position, as_transaction_id, as_transaction_ids, current_transaction_id, snapshot_in_progress_positions, snapshot_xmax};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_current_transaction_id_renders_the_xid8_function() {
        let sql = Query::select().expr(current_transaction_id()).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_current_xact_id()");
    }

    #[test]
    fn the_snapshot_ceiling_reads_the_statement_snapshot_as_a_transaction_id() {
        let sql = Query::select().expr(snapshot_xmax()).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_snapshot_xmax(pg_current_snapshot())");
    }

    #[test]
    fn the_snapshot_ceiling_reads_as_a_bigint_position() {
        let sql = Query::select().expr(as_position(snapshot_xmax())).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_snapshot_xmax(pg_current_snapshot())::text::bigint");
    }

    #[test]
    fn the_snapshot_in_progress_transactions_read_as_a_bigint_array() {
        let sql = Query::select()
            .expr(snapshot_in_progress_positions())
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT ARRAY (SELECT pg_snapshot_xip(pg_current_snapshot())::text::bigint)");
    }

    #[test]
    fn a_bigint_position_is_cast_back_to_a_transaction_id() {
        let sql = Query::select()
            .expr(as_transaction_id(Expr::val(7_i64)))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT 7::text::xid8");
    }

    #[test]
    fn bigint_positions_are_cast_back_to_transaction_ids_element_by_element() {
        let sql = Query::select()
            .expr(as_transaction_ids(Expr::val(vec![4_i64, 7])))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT ARRAY [4,7]::text[]::xid8[]");
    }
}
