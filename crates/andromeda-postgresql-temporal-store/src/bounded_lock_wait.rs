//! Running one storage-policy write without stalling the tenant behind it.
//!
//! The storage-policy reconciler writes through the tenant's serving pool,
//! while other replicas may be serving the same tenant: `ALTER TABLE ... SET`
//! takes an `AccessExclusiveLock` on the hypertable, and every statement that
//! reaches the hypertable after it queues behind that request for as long as
//! it waits. Each write therefore runs in a transaction of its own under
//! [`SCHEMA_CHANGE_LOCK_TIMEOUT`], set for that transaction only so no pooled
//! session keeps it, and is run again under [`SCHEMA_CHANGE_LOCK_RETRY`] when
//! the lock is refused.

use andromeda_postgresql_pool::schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, SCHEMA_CHANGE_LOCK_TIMEOUT, is_lock_not_available};
use andromeda_postgresql_seaquery::setting::{SettingScope, set_config};
use sea_query::{PostgresQueryBuilder, Query, SelectStatement, Values};
use sea_query_sqlx::{SqlxBinder, SqlxValues};
use sqlx::{AssertSqlSafe, PgPool};

/// Execute `sql` with `values` bound, in a transaction whose lock waits end
/// after [`SCHEMA_CHANGE_LOCK_TIMEOUT`], running it again while the lock is
/// refused.
///
/// # Errors
///
/// Returns the statement's error, or the refused lock once every attempt is
/// spent.
pub(crate) async fn execute_with_bounded_lock_wait(pool: &PgPool, sql: &str, values: &Values) -> Result<(), sqlx::Error> {
    SCHEMA_CHANGE_LOCK_RETRY
        .run(is_lock_not_available, || async {
            let mut transaction = pool.begin().await?;
            let (bound_sql, bound_values) = lock_timeout_statement().build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(AssertSqlSafe(bound_sql), bound_values).execute(&mut *transaction).await?;
            sqlx::query_with(AssertSqlSafe(sql), SqlxValues(values.clone()))
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await
        })
        .await
}

/// `SELECT set_config('lock_timeout', '<n>ms', true)`.
pub(crate) fn lock_timeout_statement() -> SelectStatement {
    Query::select()
        .expr(set_config(
            "lock_timeout",
            &format!("{}ms", SCHEMA_CHANGE_LOCK_TIMEOUT.as_millis()),
            SettingScope::Transaction,
        ))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::bounded_lock_wait::lock_timeout_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_lock_timeout_lasts_until_the_write_transaction_ends() {
        assert_eq!(
            lock_timeout_statement().to_string(PostgresQueryBuilder),
            "SELECT set_config('lock_timeout', '2000ms', TRUE)"
        );
    }
}
