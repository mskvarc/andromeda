//! The tenant schema stamp a name cache is warmed against.
//!
//! A tenant schema dropped and recreated under the same name comes back with
//! fresh `IDENTITY` sequences, so every name->key mapping a cache published
//! against the previous generation points at a dictionary row that no longer
//! exists. `schema_metadata.applied_at` is re-stamped by every fresh migration,
//! so it is the generation marker a cache compares against to notice that.

use crate::name_lookup::NameLookupError;
use andromeda_postgresql_schema::tenancy::SchemaMetadata;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Query, SelectStatement};
use sqlx::{Executor, Postgres, postgres::PgPool};
use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

/// The instant the epoch throttle measures elapsed time from. A process-wide
/// monotonic origin lets the last-checked stamp be a relaxed atomic store
/// rather than a lock.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Milliseconds elapsed since [`EPOCH`].
///
/// `Duration::as_millis` reports a `u128`; the origin is a process-monotonic
/// instant, so the elapsed value cannot approach the `u64` bound and saturating
/// there is unreachable rather than a silent narrowing.
pub(crate) fn now_millis() -> u64 {
    u64::try_from(EPOCH.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The sentinel for a schema epoch not yet read. `schema_metadata.applied_at`
/// is a wall-clock instant that only ever advances, so this floor cannot
/// collide with an observed stamp.
pub(crate) const UNKNOWN_EPOCH: i64 = i64::MIN;

/// How long a resolution trusts the established schema epoch before it re-reads
/// the tenant schema stamp to detect an out-of-band drop and recreate. The
/// window bounds how long a stale cache can serve keys of a previous schema
/// generation; the broker itself performs no drop, so only operator surgery can
/// open it, and five seconds keeps the steady-state catalogue cost negligible.
pub(crate) const EPOCH_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// The tenant schema stamp: `SELECT applied_at FROM schema_metadata WHERE
/// singleton`, the instant the broker last stamped this schema's contract.
fn schema_epoch_query() -> SelectStatement {
    Query::select()
        .column(SchemaMetadata::AppliedAt)
        .from(SchemaMetadata::Table)
        .and_where(Expr::col(SchemaMetadata::Singleton).eq(true))
        .to_owned()
}

/// Read the tenant schema stamp on the given executor, as microseconds since
/// the Unix epoch, or `None` while the schema is migrated but not yet stamped.
///
/// Taken as an executor rather than always the pool so a minting resolution can
/// probe on the connection its transaction already holds, never reaching into
/// the pool for a second connection mid-commit.
pub(crate) async fn read_schema_epoch<'e, E: Executor<'e, Database = Postgres>>(executor: E) -> Result<Option<i64>, NameLookupError> {
    let stamped: Option<DateTime<Utc>> = schema_epoch_query().to_query_scalar().fetch_optional(executor).await?;
    Ok(stamped.map(|applied_at| applied_at.timestamp_micros()))
}

/// Read the tenant schema stamp once over the tenant pool, for a caller warming
/// several dictionaries against the same database so they share one read rather
/// than each issuing its own. Feed the result to
/// [`NameCache::adopt_epoch`](crate::name_cache::NameCache::adopt_epoch).
///
/// # Errors
///
/// Returns [`NameLookupError::Statement`] when the stamp read fails.
pub async fn read_established_epoch(pool: &PgPool) -> Result<Option<i64>, NameLookupError> {
    read_schema_epoch(pool).await
}

#[cfg(test)]
mod tests {
    use crate::schema_epoch::{UNKNOWN_EPOCH, now_millis, schema_epoch_query};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_schema_stamp_is_read_from_the_singleton_metadata_row() {
        let sql = schema_epoch_query().to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT \"applied_at\" FROM \"schema_metadata\" WHERE \"singleton\" = TRUE");
    }

    #[test]
    fn the_monotonic_clock_never_reads_as_the_unknown_epoch_sentinel() {
        let now = now_millis();
        assert!(i64::try_from(now).is_ok_and(|millis| millis != UNKNOWN_EPOCH));
    }
}
