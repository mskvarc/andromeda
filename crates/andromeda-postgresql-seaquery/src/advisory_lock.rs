//! The transaction-scoped advisory lock a serialized write path takes.
//!
//! PostgreSQL advisory locks are keyed on an integer, so a text key is folded
//! to one with `hashtext`. sea-query has no constructor for either function, so
//! the call is built here rather than as a hand-written fragment at the store.

use sea_query::{Expr, Func, Iden, SimpleExpr};

/// The lock and hashing functions an advisory lock is built from.
#[derive(Iden, Clone, Copy)]
enum LockFunction {
    /// `pg_advisory_xact_lock(key)`.
    #[iden = "pg_advisory_xact_lock"]
    AdvisoryXactLock,
    /// `pg_try_advisory_xact_lock(key)`.
    #[iden = "pg_try_advisory_xact_lock"]
    TryAdvisoryXactLock,
    /// `pg_try_advisory_lock(key)`.
    #[iden = "pg_try_advisory_lock"]
    TryAdvisoryLock,
    /// `pg_advisory_unlock(key)`.
    #[iden = "pg_advisory_unlock"]
    AdvisoryUnlock,
    /// `pg_advisory_lock(key)`.
    #[iden = "pg_advisory_lock"]
    AdvisoryLock,
    /// `pg_advisory_lock_shared(key)`.
    #[iden = "pg_advisory_lock_shared"]
    AdvisoryLockShared,
    /// `pg_advisory_unlock_shared(key)`.
    #[iden = "pg_advisory_unlock_shared"]
    AdvisoryUnlockShared,
    /// `hashtext(text)`.
    #[iden = "hashtext"]
    Hashtext,
}

/// `pg_try_advisory_lock(hashtext(key))` - obtain a session lock without waiting.
#[must_use]
pub fn try_advisory_lock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::TryAdvisoryLock).args([hashed]).into()
}

/// `pg_advisory_unlock(hashtext(key))` - release a held session lock.
#[must_use]
pub fn advisory_unlock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::AdvisoryUnlock).args([hashed]).into()
}

/// `pg_advisory_xact_lock(hashtext(key))` - take a transaction advisory lock
/// keyed on the hash of `key`, released when the transaction ends.
#[must_use]
pub fn advisory_xact_lock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::AdvisoryXactLock).args([hashed]).into()
}

/// `pg_try_advisory_xact_lock(hashtext(key))` - take a transaction advisory
/// lock without waiting, released when the transaction ends.
#[must_use]
pub fn try_advisory_xact_lock(key: SimpleExpr) -> SimpleExpr {
    let hashed = SimpleExpr::from(Func::cust(LockFunction::Hashtext).args([key]));
    Func::cust(LockFunction::TryAdvisoryXactLock).args([hashed]).into()
}

/// `pg_advisory_lock(key)` - take the exclusive session lock on the integer
/// `key` as it is, waiting until no other session holds it in either mode.
#[must_use]
pub fn advisory_lock_exclusive(key: i64) -> SimpleExpr {
    Func::cust(LockFunction::AdvisoryLock).args([SimpleExpr::from(Expr::val(key))]).into()
}

/// `pg_advisory_lock_shared(key)` - take the shared session lock on the
/// integer `key` as it is, waiting while another session holds it exclusively.
#[must_use]
pub fn advisory_lock_shared(key: i64) -> SimpleExpr {
    Func::cust(LockFunction::AdvisoryLockShared).args([SimpleExpr::from(Expr::val(key))]).into()
}

/// `pg_advisory_unlock(key)` - release the exclusive session lock on the
/// integer `key` as it is; `false` when the session held none.
#[must_use]
pub fn advisory_unlock_exclusive(key: i64) -> SimpleExpr {
    Func::cust(LockFunction::AdvisoryUnlock).args([SimpleExpr::from(Expr::val(key))]).into()
}

/// `pg_advisory_unlock_shared(key)` - release the shared session lock on the
/// integer `key` as it is; `false` when the session held none.
#[must_use]
pub fn advisory_unlock_shared(key: i64) -> SimpleExpr {
    Func::cust(LockFunction::AdvisoryUnlockShared).args([SimpleExpr::from(Expr::val(key))]).into()
}

#[cfg(test)]
mod tests {
    use crate::advisory_lock::{
        advisory_lock_exclusive,
        advisory_lock_shared,
        advisory_unlock,
        advisory_unlock_exclusive,
        advisory_unlock_shared,
        advisory_xact_lock,
        try_advisory_lock,
        try_advisory_xact_lock,
    };
    use pretty_assertions::assert_eq;
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
    fn an_integer_keyed_session_lock_binds_the_key_unhashed() {
        for (expression, function) in [
            (advisory_lock_exclusive(7), "pg_advisory_lock($1)"),
            (advisory_lock_shared(7), "pg_advisory_lock_shared($1)"),
            (advisory_unlock_exclusive(7), "pg_advisory_unlock($1)"),
            (advisory_unlock_shared(7), "pg_advisory_unlock_shared($1)"),
        ] {
            let (sql, values) = Query::select().expr(expression).to_owned().build(PostgresQueryBuilder);
            assert_eq!(sql, format!("SELECT {function}"));
            assert_eq!(values.0.len(), 1, "{sql}");
        }
    }

    #[test]
    fn session_lock_calls_bind_and_hash_the_key() {
        for expression in [
            try_advisory_lock(Expr::val("fleet")),
            advisory_unlock(Expr::val("fleet")),
            try_advisory_xact_lock(Expr::val("fleet")),
        ] {
            let (sql, values) = Query::select().expr(expression).to_owned().build(PostgresQueryBuilder);
            assert!(sql.contains("hashtext($1)"), "{sql}");
            assert_eq!(values.0.len(), 1, "{sql}");
        }
    }
}
