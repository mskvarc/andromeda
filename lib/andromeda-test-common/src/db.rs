//! Database pool + cleanup helpers.
//!
//! One shared [`sqlx::PgPool`] per process is used by [`crate::scope::TestScope`]
//! for scoped row deletion. The `serial(global)` fallback still calls
//! [`truncate_all`] for the handful of tests that must start from an empty
//! broker.

use crate::config::DATABASE_URL;
use sqlx::PgPool;
use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{LazyLock, OnceLock},
    thread::available_parallelism,
};
use tokio::sync::Mutex;

static POOL: OnceLock<Mutex<Option<PgPool>>> = OnceLock::new();

/// SQL executed by [`truncate_all`] — drops every row in every tenant-scoped
/// table. Reserved for the `serial(global)` path; scoped tests must use the
/// per-prefix cleanup in [`crate::scope::TestScope::cleanup`] instead.
///
/// The table list is read from the tenant migrations rather than hand-written,
/// so a migration that adds a table has that table truncated automatically. A
/// hand-copied list silently under-covered — it named seven of the fourteen
/// tables, so rows in the others (the outbox, the scope history, the
/// materialised current-attribute table, …) leaked between `serial(global)`
/// tests.
pub static TRUNCATE_ALL: LazyLock<String> = LazyLock::new(|| format!("TRUNCATE {} RESTART IDENTITY CASCADE", tenant_tables().join(", ")));

/// Dictionary tables the broker mirrors in an in-memory cache, which a truncate
/// must leave intact.
///
/// `attribute_names` maps each attribute name to a small integer id that
/// `temporal_entity_attributes` and `attribute_current` store by foreign key.
/// The broker caches that mapping and reuses a cached id without re-checking the
/// row is still there, so truncating the table (or resetting its identity)
/// strands the cache: the next attribute write fails the `attribute_name_id`
/// foreign key with an id the table no longer holds. Every other tenant table
/// holds only rows a test created, cascades from `entities`, or both, so it is
/// safe to empty between `serial(global)` runs.
const CACHED_DICTIONARIES: &[&str] = &["attribute_names"];

/// Every tenant-scoped table a truncate may empty, in migration order, read
/// from the `CREATE TABLE` statements of `migrations/tenant/*.sql`.
///
/// Only `CREATE TABLE` is matched, so continuous aggregates and other objects a
/// `TRUNCATE` cannot target are excluded by construction, and the
/// [`CACHED_DICTIONARIES`] the broker mirrors are filtered out. `test-common`
/// sits at a fixed depth under the workspace root, so the relative path to the
/// shared migrations is stable.
fn tenant_tables() -> Vec<String> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations/tenant");
    let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
        .unwrap_or_else(|error| unreachable!("{} is readable: {error}", directory.display()))
        .map(|entry| entry.unwrap_or_else(|error| unreachable!("directory entry is readable: {error}")).path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    paths.sort();

    let mut tables = Vec::new();
    for path in paths {
        let sql = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
        for name in created_tables(&sql) {
            if !CACHED_DICTIONARIES.contains(&name.as_str()) && !tables.contains(&name) {
                tables.push(name);
            }
        }
    }
    tables
}

/// The table names one migration's `CREATE TABLE [IF NOT EXISTS] <name>`
/// statements declare.
fn created_tables(sql: &str) -> Vec<String> {
    const MARKER: &str = "CREATE TABLE";
    let mut names = Vec::new();
    let mut rest = sql;
    while let Some(offset) = rest.find(MARKER) {
        let after = rest[offset + MARKER.len()..].trim_start();
        let after = after.strip_prefix("IF NOT EXISTS ").unwrap_or(after);
        let name: String = after.chars().take_while(|character| !character.is_whitespace() && *character != '(').collect();
        let name = name.trim_matches('"');
        if !name.is_empty() {
            names.push(name.to_owned());
        }
        rest = &rest[offset + MARKER.len()..];
    }
    names
}

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
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await?;
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

#[cfg(test)]
mod tests {
    use crate::db::tenant_tables;

    #[test]
    fn the_derived_truncate_set_covers_the_old_list_and_the_tables_it_silently_missed() {
        let tables = tenant_tables();

        // The seven the hand-written const named.
        for named in [
            "entities",
            "jsonld_contexts",
            "temporal_entity_attributes",
            "attribute_history",
            "subscriptions",
            "context_source_registrations",
            "snapshots",
        ] {
            assert!(tables.iter().any(|table| table == named), "derived set must keep {named}");
        }

        // The state tables the const omitted, whose rows leaked between serial tests.
        for missing in ["scope_history", "notification_outbox", "attribute_current", "pending_side_effects"] {
            assert!(tables.iter().any(|table| table == missing), "derived set must now include {missing}");
        }

        // The dictionary the broker mirrors in memory must be left intact.
        assert!(
            !tables.iter().any(|table| table == "attribute_names"),
            "attribute_names must not be truncated: the broker caches its name->id mapping"
        );
    }
}
