//! The transaction-scoped advisory lock a serialized write path takes.
//!
//! PostgreSQL advisory locks are keyed on an integer, so a text key is folded
//! to one with `hashtext`. sea-query has no constructor for either function, so
//! the call is built here rather than as a hand-written fragment at the store.

use sea_query::{Func, Iden, SimpleExpr};

/// The lock and hashing functions an advisory lock is built from.
#[derive(Iden, Clone, Copy)]
enum LockFunction {
    /// `pg_advisory_xact_lock(key)`.
    #[iden = "pg_advisory_xact_lock"]
    AdvisoryXactLock,
    /// `pg_try_advisory_lock(key)`.
    #[iden = "pg_try_advisory_lock"]
    TryAdvisoryLock,
    /// `pg_advisory_unlock(key)`.
    #[iden = "pg_advisory_unlock"]
    AdvisoryUnlock,
    /// `hashtext(text)`.
    #[iden = "hashtext"]
    Hashtext,
}

/// `pg_try_advisory_lock(hashtext(key))` - obtain a session lock without waiting.
pub fn try_advisory_lock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::TryAdvisoryLock).args([hashed]).into()
}

/// `pg_advisory_unlock(hashtext(key))` - release a held session lock.
pub fn advisory_unlock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::AdvisoryUnlock).args([hashed]).into()
}

/// `pg_advisory_xact_lock(hashtext(key))` - take a transaction advisory lock
/// keyed on the hash of `key`, released when the transaction ends.
pub fn advisory_xact_lock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::AdvisoryXactLock).args([hashed]).into()
}

#[cfg(test)]
mod tests {
    use crate::advisory_lock::{advisory_unlock, advisory_xact_lock, try_advisory_lock};
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_lock_key_is_the_hash_of_the_bound_value() {
        let (sql, values) = Query::select()
            .expr(advisory_xact_lock(Expr::val("tenant-a")))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        // The key is a bound value, not text spliced into the statement.
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn session_lock_calls_bind_and_hash_the_key() {
        for expression in [try_advisory_lock(Expr::val("fleet")), advisory_unlock(Expr::val("fleet"))] {
            let (sql, values) = Query::select().expr(expression).to_owned().build(PostgresQueryBuilder);
            assert!(sql.contains("hashtext($1)"), "{sql}");
            assert_eq!(values.0.len(), 1, "{sql}");
        }
    }
}
