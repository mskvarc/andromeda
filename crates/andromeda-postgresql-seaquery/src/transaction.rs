//! PostgreSQL transaction-control statements not represented by SeaQuery.

/// Start the current transaction at repeatable-read isolation.
pub const fn repeatable_read() -> &'static str {
    "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
}
