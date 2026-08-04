//! Database pool + cleanup helpers.
//!
//! One shared [`sqlx::PgPool`] per process is used by [`crate::scope::TestScope`]
//! for scoped row deletion. The `serial(global)` fallback still calls
//! [`truncate_all`] for the handful of tests that must start from an empty
//! broker.

use crate::config::DATABASE_URL;
use sqlx::PgPool;
use std::{num::NonZeroUsize, sync::OnceLock, thread::available_parallelism};
use tokio::sync::Mutex;

static POOL: OnceLock<Mutex<Option<PgPool>>> = OnceLock::new();

/// SQL executed by [`truncate_all`] — drops every row in every tenant-scoped
/// table. Reserved for the `serial(global)` path; scoped tests must use the
/// per-prefix cleanup in [`crate::scope::TestScope::cleanup`] instead.
pub const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

/// Open (once per process) and return a handle to the shared test database pool.
///
/// The pool is created with `min_connections = 0` so idle tests do not hold
/// connections, and `max_connections` defaults to `CPU * 4` which is enough
/// headroom for the libtest parallel worker pool.
pub async fn pool() -> Result<PgPool, sqlx::Error> {
    let slot = POOL.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().await;
    if let Some(pool) = guard.as_ref() {
        return Ok(pool.clone());
    }
    let max_connections = (num_cpus().saturating_mul(4)).max(8) as u32;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .min_connections(0)
        .max_connections(max_connections)
        .connect(DATABASE_URL)
        .await?;
    *guard = Some(pool.clone());
    Ok(pool)
}

fn num_cpus() -> usize {
    available_parallelism().map_or(4, NonZeroUsize::get)
}

/// Truncate every tenant-scoped table in the default tenant database.
///
/// Only tests marked `#[serial(global)]` should call this. Regular parallel
/// tests rely on [`crate::scope::TestScope::cleanup`] instead.
pub async fn truncate_all() -> Result<(), sqlx::Error> {
    let pool = pool().await?;
    sqlx::query(TRUNCATE_ALL).execute(&pool).await?;
    Ok(())
}

/// Delete every row whose id starts with `prefix` from each tenant-scoped
/// table. Called by [`crate::scope::TestScope::cleanup`].
///
/// `temporal_entity_attributes` and `attribute_history` are reached through
/// the `ON DELETE CASCADE` on `entities.id`, so only the four top-level
/// tables need explicit statements.
pub async fn delete_by_prefix(prefix: &str) -> Result<(), sqlx::Error> {
    let pool = pool().await?;
    let like = format!("{prefix}%");
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM entities WHERE id LIKE $1").bind(&like).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM subscriptions WHERE id LIKE $1").bind(&like).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM context_source_registrations WHERE id LIKE $1")
        .bind(&like)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM snapshots WHERE id LIKE $1").bind(&like).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
