//! The advisory lock that keeps a Snapshot's branch schema in place while
//! PostgreSQL maintenance works in it.
//!
//! Maintenance holds it as a session lock while it probes, vacuums, rebuilds
//! or recovers anything in a published branch. A reap tries it inside its
//! transaction before dropping the branch schemas. Neither side ever waits for
//! it: a `DROP SCHEMA ... CASCADE` queued behind a concurrent rebuild would
//! block every later reader of the branch, so a reap that loses the lock still
//! deletes the control record and leaves the schemas to the orphan sweep, and
//! maintenance skips a branch whose lock a reap holds.

use andromeda_context_space::snapshot_key::SnapshotKey;
use sea_query::{Expr, SimpleExpr};

/// The key of one Snapshot's branch lock, a text value the advisory lock
/// functions hash.
#[must_use]
pub fn branch_lock_key(key: SnapshotKey) -> SimpleExpr {
    Expr::val(format!("andromeda.snapshot.branch.{}", key.as_i64()))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::snapshot::branch_lock::branch_lock_key;
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn each_snapshot_has_its_own_branch_lock_key() {
        let render = |raw: i64| {
            Query::select()
                .expr(branch_lock_key(SnapshotKey::try_from(raw).expect("positive snapshot key")))
                .to_owned()
                .to_string(PostgresQueryBuilder)
        };
        assert_eq!(render(42), "SELECT 'andromeda.snapshot.branch.42'");
        assert_ne!(render(42), render(4));
    }
}
