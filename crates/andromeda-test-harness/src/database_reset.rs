//! Authoritative global reset and complete prefix-owned cleanup.

use crate::{
    database_isolation::{DatabaseIsolationError, SerializedLease},
    discovery_census::rebuild_live_census,
    identifiers::TestIdentifiers,
};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use std::time::Duration;
use thiserror::Error;
use tokio::time::{Instant, sleep};

/// Authoritative tenant reset SQL maintained alongside the test harness.
pub const RESET_TENANT_SQL: &str = include_str!("../sql/reset-tenant.sql");

/// Reset every mutable tenant table while preserving the cached `attribute_names` and `entity_type_names` dictionaries, the `schema_metadata` contract row, and the seeded unit-code catalogue.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Isolation`] when the exclusive lease no longer holds its connection, and [`DatabaseResetError::Sqlx`] when the reset script fails for any reason other than a busy table, or when a table stays busy past [`RESET_BUSY_DEADLINE`].
pub async fn reset_tenant(lease: &mut SerializedLease) -> Result<(), DatabaseResetError> {
    let busy = BusyTables::starting_now();
    loop {
        let connection = lease.connection()?;
        match connection.execute(RESET_TENANT_SQL).await {
            Ok(_) => return Ok(()),
            Err(error) => busy.wait_out(error).await?,
        }
    }
}

/// Reset a tenant database the calling suite created and owns outright.
///
/// A suite that creates its own tenant database holds no advisory lock over it,
/// because no other suite ever connects to it. The reset therefore runs on the
/// suite's own pool instead of the exclusive lease the shared test database
/// requires, and preserves exactly the same migration-owned relations.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when the reset script fails for any reason other than a busy table, or when a table stays busy past [`RESET_BUSY_DEADLINE`].
pub async fn reset_owned_tenant(pool: &PgPool) -> Result<(), DatabaseResetError> {
    let busy = BusyTables::starting_now();
    loop {
        match pool.execute(RESET_TENANT_SQL).await {
            Ok(_) => return Ok(()),
            Err(error) => busy.wait_out(error).await?,
        }
    }
}

/// How long a reset keeps retrying while a broker transaction holds one of its
/// tables. The brokers' background transactions (expiry sweeps, notification
/// drains) are short, so a free moment comes far sooner than this.
const RESET_BUSY_DEADLINE: Duration = Duration::from_secs(30);

/// Pause before a reset that found a table busy is run again.
const RESET_BUSY_BACKOFF: Duration = Duration::from_millis(20);

/// PostgreSQL SQLSTATE for `lock_not_available`, which the reset script's
/// `LOCK TABLE ... NOWAIT` raises when a broker transaction holds a table.
const LOCK_NOT_AVAILABLE_SQLSTATE: &str = "55P03";

/// The retry policy of one reset: the script takes every lock it needs at once
/// and without waiting, so a broker transaction still open on a table fails it
/// before it has changed or locked anything, and it is simply run again.
///
/// Only that failure is retried. A deadlock cannot involve a script that never
/// waits for a lock while holding one, so it is reported rather than retried,
/// as is every other failure.
struct BusyTables {
    /// When the reset stops waiting for its tables to become free.
    deadline: Instant,
}

impl BusyTables {
    /// A retry policy whose deadline runs from now.
    fn starting_now() -> BusyTables {
        BusyTables {
            deadline: Instant::now() + RESET_BUSY_DEADLINE,
        }
    }

    /// Wait out `error` when it is a busy table and the deadline has not
    /// passed, so the caller runs the reset again; otherwise return it.
    async fn wait_out(&self, error: sqlx::Error) -> Result<(), DatabaseResetError> {
        if is_lock_not_available(&error) && Instant::now() < self.deadline {
            sleep(RESET_BUSY_BACKOFF).await;
            return Ok(());
        }
        Err(DatabaseResetError::from_sqlx(error))
    }
}

/// Whether a database error is `LOCK TABLE ... NOWAIT` finding a table busy.
fn is_lock_not_available(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.code().is_some_and(|code| code == LOCK_NOT_AVAILABLE_SQLSTATE))
}

/// Every prefix-scoped delete a cleanup runs, in dependency order.
///
/// Temporal Evolution is reached through `temporal_entities`, not through
/// `entities`: the rebuild removed the foreign key that once made deleting an
/// entity cascade into its history, so an entity delete now reclaims nothing
/// temporal. `temporal_attribute_streams` cascades from `temporal_entities`,
/// while `attribute_history` and `scope_history` carry no foreign key at all —
/// hypertables cannot — so each is deleted through the streams and the
/// catalogue that name it, before the rows those names live on are removed.
///
/// `attribute_history_instance_claims` is absent deliberately: it is an
/// ordinary table whose foreign key cascades from `temporal_entities`, so the
/// catalogue delete below reclaims every claim the prefix staked. The global
/// reset still names it, because a `TRUNCATE` list is not a cascade.
const PREFIX_DELETES: &[&str] = &[
    // `notification_events` carries the Entity id as text and has no foreign key
    // to `subscriptions` -- a subscription delete cascades its deliveries but
    // never the events -- so it is reclaimed here by the Entity-id prefix rather
    // than through any cascade.
    "DELETE FROM notification_events WHERE entity_id LIKE $1",
    "DELETE FROM subscriptions WHERE id LIKE $1",
    "DELETE FROM context_source_registrations WHERE id LIKE $1",
    "DELETE FROM snapshots WHERE id LIKE $1",
    "DELETE FROM entities WHERE id LIKE $1",
    "DELETE FROM attribute_history WHERE stream_key IN (SELECT s.stream_key FROM temporal_attribute_streams s \
     JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key WHERE te.id LIKE $1)",
    "DELETE FROM scope_history WHERE temporal_entity_key IN (SELECT temporal_entity_key FROM temporal_entities WHERE id LIKE $1)",
    "DELETE FROM temporal_entities WHERE id LIKE $1",
];

/// Delete every row discoverably owned by one isolated identifier prefix.
///
/// The Entities go by a raw delete the broker's discovery census never sees,
/// so the live census is rebuilt from the search rows that remain, in the same
/// transaction (see [`rebuild_live_census`]).
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when the transaction cannot be opened, when any prefix-scoped delete fails, or when the commit fails.
pub async fn cleanup_prefix(pool: &PgPool, identifiers: &TestIdentifiers) -> Result<(), DatabaseResetError> {
    let like = format!("{}%", identifiers.prefix());
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::from_sqlx)?;
    delete_temporal_intents(&mut transaction, &like).await?;
    for statement in PREFIX_DELETES {
        sqlx::query(*statement)
            .bind(&like)
            .execute(&mut *transaction)
            .await
            .map_err(DatabaseResetError::from_sqlx)?;
    }
    rebuild_live_census(&mut transaction).await.map_err(DatabaseResetError::from_sqlx)?;
    transaction.commit().await.map_err(DatabaseResetError::from_sqlx)?;
    Ok(())
}

async fn delete_temporal_intents(transaction: &mut Transaction<'_, Postgres>, like: &str) -> Result<(), DatabaseResetError> {
    sqlx::query("DELETE FROM write_records WHERE write_id IN (SELECT write_id FROM temporal_append_intents WHERE entity_id LIKE $1)")
        .bind(like)
        .execute(&mut **transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    sqlx::query("DELETE FROM temporal_append_intents WHERE entity_id LIKE $1")
        .bind(like)
        .execute(&mut **transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    Ok(())
}

/// Global reset or isolated cleanup failure.
#[derive(Debug, Error)]
pub enum DatabaseResetError {
    /// The serialized-state capability was unavailable.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// `SQLx` could not execute the reset or cleanup transaction.
    ///
    /// Boxed for the same reason as [`DatabaseIsolationError::Sqlx`]: an
    /// `sqlx::Error` is 56 bytes, and this enum is the `Err` of every reset and
    /// prefix cleanup the harness runs.
    #[error("test database reset failed: {0}")]
    Sqlx(#[source] Box<sqlx::Error>),
}

impl DatabaseResetError {
    /// Record an `SQLx` failure, moving it behind the variant's box.
    #[must_use]
    pub fn from_sqlx(error: sqlx::Error) -> DatabaseResetError {
        DatabaseResetError::Sqlx(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use crate::database_reset::{PREFIX_DELETES, RESET_TENANT_SQL, is_lock_not_available};

    #[test]
    fn a_non_database_error_is_not_treated_as_a_busy_table() {
        assert!(!is_lock_not_available(&sqlx::Error::PoolClosed));
        assert!(!is_lock_not_available(&sqlx::Error::RowNotFound));
    }

    /// The reset relation list of the script, without the header comment that
    /// explains which relations are deliberately absent from it.
    fn truncated_relations() -> &'static str {
        let (_, declaration) = RESET_TENANT_SQL
            .split_once("reset_relations CONSTANT regclass[] := ARRAY[")
            .unwrap_or_else(|| panic!("the reset script declares its relations"));
        let (relations, _) = declaration.split_once("]::regclass[]").unwrap_or_else(|| panic!("the relation list is closed"));
        relations
    }

    /// The script must take every lock before it changes anything, and never
    /// wait for one: that is what keeps it out of every deadlock with the
    /// brokers' own transactions.
    #[test]
    fn reset_script_locks_everything_it_touches_without_waiting_before_it_truncates() {
        let lock = RESET_TENANT_SQL
            .find("IN ACCESS EXCLUSIVE MODE NOWAIT")
            .unwrap_or_else(|| panic!("the script locks without waiting"));
        let truncate = RESET_TENANT_SQL
            .find("EXECUTE format('TRUNCATE")
            .unwrap_or_else(|| panic!("the script truncates"));
        let drop = RESET_TENANT_SQL
            .find("DROP SCHEMA")
            .unwrap_or_else(|| panic!("the script drops the branch schemas"));
        assert!(lock < truncate && lock < drop, "the locks must precede every change");
        assert!(
            RESET_TENANT_SQL.contains("constraint_row.contype = 'f'"),
            "the lock covers what the cascade reaches"
        );
        assert!(RESET_TENANT_SQL.contains("TRUNCATE %s RESTART IDENTITY CASCADE"));
    }

    #[test]
    fn reset_script_covers_mutable_tables_and_preserves_cached_dictionaries() {
        for table in [
            "entities",
            "entity_documents",
            "entity_search",
            "entity_type_members",
            "jsonld_contexts",
            "temporal_entities",
            "temporal_attribute_streams",
            "attribute_history",
            "attribute_history_instance_claims",
            "subscriptions",
            "subscription_delivery_state",
            "context_source_registrations",
            "registration_delivery_state",
            "registration_information",
            "subscription_companions",
            "forwarded_subscriptions",
            "snapshots",
            "snapshot_preparation",
            "snapshot_notification_outbox",
            "scope_history",
            "notification_deliveries",
            "notification_events",
            "current_attribute_values",
            "pending_side_effects",
            "temporal_append_intents",
            "write_records",
        ] {
            assert!(truncated_relations().contains(table), "reset SQL must cover {table}");
        }
        assert!(!truncated_relations().contains("attribute_names"));
        assert!(!truncated_relations().contains("entity_type_names"));
    }

    /// The rebuild detached Temporal Evolution from current entity state, so
    /// prefix cleanup has to name every temporal relation itself; relying on a
    /// cascade from `entities` would leak history across runs.
    #[test]
    fn prefix_cleanup_reaches_every_relation_the_reset_script_truncates() {
        let deletes = PREFIX_DELETES.join(" ");
        for relation in [
            "notification_events",
            "subscriptions",
            "context_source_registrations",
            "snapshots",
            "entities",
            "attribute_history",
            "scope_history",
            "temporal_entities",
        ] {
            assert!(deletes.contains(relation), "prefix cleanup must delete from {relation}");
        }
    }

    /// `temporal_entities` is deleted last so the statements that locate
    /// history rows through it still find their catalogue rows.
    #[test]
    fn prefix_cleanup_removes_the_temporal_catalogue_after_the_rows_that_name_it() {
        let catalogue = PREFIX_DELETES
            .iter()
            .position(|statement| statement.starts_with("DELETE FROM temporal_entities"))
            .unwrap_or_else(|| panic!("the catalogue is deleted"));
        for relation in ["DELETE FROM attribute_history", "DELETE FROM scope_history"] {
            let dependent = PREFIX_DELETES
                .iter()
                .position(|statement| statement.starts_with(relation))
                .unwrap_or_else(|| panic!("{relation} is deleted"));
            assert!(dependent < catalogue, "{relation} must run before the catalogue it reads");
        }
    }

    #[test]
    fn reset_script_leaves_the_seeded_unit_code_catalogue_in_place() {
        // The catalogue is migration-seeded reference data, and
        // `current_attribute_values.unit_code_id` references it.
        assert!(!truncated_relations().contains("unit_codes"));
        assert!(!truncated_relations().contains("unit_code_catalog_metadata"));
    }

    #[test]
    fn reset_script_drops_every_snapshot_branch_schema_and_keeps_the_live_default_schema() {
        // Each Snapshot branch is a `snap_<key>` schema; the reset drops them all
        // and never references the removed `context_spaces` table.
        assert!(RESET_TENANT_SQL.contains("DROP SCHEMA"));
        assert!(RESET_TENANT_SQL.contains(r"snap\_%"));
        assert!(!RESET_TENANT_SQL.contains("context_spaces"));
    }

    #[test]
    fn reset_script_leaves_the_schema_contract_singleton_in_place() {
        // Truncating `schema_metadata` makes the tenant read as never-stamped,
        // and the startup verification refuses every connection after it.
        assert!(!truncated_relations().contains("schema_metadata"));
    }
}
