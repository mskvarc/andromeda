//! Authoritative global reset and complete prefix-owned cleanup.

use crate::{
    database_isolation::{DatabaseIsolationError, SerializedLease},
    identifiers::TestIdentifiers,
};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;

/// Authoritative tenant reset SQL maintained alongside the test harness.
pub const RESET_TENANT_SQL: &str = include_str!("../sql/reset-tenant.sql");

/// Reset every mutable tenant table while preserving the cached `attribute_names` and `entity_type_names` dictionaries, the `schema_metadata` contract row, and the seeded unit-code catalogue.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Isolation`] when the exclusive lease no longer holds its connection, and [`DatabaseResetError::Sqlx`] when the reset script fails.
pub async fn reset_tenant(lease: &mut SerializedLease) -> Result<(), DatabaseResetError> {
    let connection = lease.connection()?;
    connection.execute(RESET_TENANT_SQL).await.map_err(DatabaseResetError::Sqlx)?;
    Ok(())
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
/// Returns [`DatabaseResetError::Sqlx`] when the reset script fails for any reason other than a deadlock, or when it still deadlocks after the bounded deadlock retries are exhausted.
pub async fn reset_owned_tenant(pool: &PgPool) -> Result<(), DatabaseResetError> {
    let mut attempt = 0;
    loop {
        match pool.execute(RESET_TENANT_SQL).await {
            Ok(_) => return Ok(()),
            Err(error) if is_deadlock(&error) && attempt < RESET_DEADLOCK_RETRIES => {
                // The brokers' background tasks (expiry sweeps, notification
                // drains) keep short transactions open against the same
                // tables; a TRUNCATE racing one of them is reported as a
                // deadlock rather than waiting, so the reset is simply tried
                // again once that transaction has finished.
                attempt += 1;
                sleep(RESET_DEADLOCK_BACKOFF).await;
            }
            Err(error) => return Err(DatabaseResetError::Sqlx(error)),
        }
    }
}

/// How many times a reset is retried after PostgreSQL reports a deadlock.
const RESET_DEADLOCK_RETRIES: u32 = 20;

/// Pause between deadlock retries, long enough for a broker's short
/// maintenance transaction to complete.
const RESET_DEADLOCK_BACKOFF: Duration = Duration::from_millis(200);

/// PostgreSQL SQLSTATE for `deadlock_detected`.
const DEADLOCK_SQLSTATE: &str = "40P01";

/// Whether a database error is PostgreSQL's deadlock report.
fn is_deadlock(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.code().is_some_and(|code| code == DEADLOCK_SQLSTATE))
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
    "DELETE FROM subscriptions WHERE id LIKE $1",
    "DELETE FROM context_source_registrations WHERE id LIKE $1",
    "DELETE FROM snapshots WHERE id LIKE $1",
    "DELETE FROM entities WHERE id LIKE $1",
    "DELETE FROM attribute_history WHERE stream_key IN (SELECT s.stream_key FROM temporal_attribute_streams s \
     JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key WHERE te.id LIKE $1)",
    "DELETE FROM scope_history WHERE temporal_entity_key IN (SELECT entity_key FROM temporal_entities WHERE id LIKE $1)",
    "DELETE FROM temporal_entities WHERE id LIKE $1",
];

/// Delete every row discoverably owned by one isolated identifier prefix.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when the transaction cannot be opened, when any prefix-scoped delete fails, or when the commit fails.
pub async fn cleanup_prefix(pool: &PgPool, identifiers: &TestIdentifiers) -> Result<(), DatabaseResetError> {
    let like = format!("{}%", identifiers.prefix());
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::Sqlx)?;
    delete_temporal_intents(&mut transaction, &like).await?;
    for statement in PREFIX_DELETES {
        sqlx::query(*statement)
            .bind(&like)
            .execute(&mut *transaction)
            .await
            .map_err(DatabaseResetError::Sqlx)?;
    }
    transaction.commit().await.map_err(DatabaseResetError::Sqlx)?;
    Ok(())
}

async fn delete_temporal_intents(transaction: &mut Transaction<'_, Postgres>, like: &str) -> Result<(), DatabaseResetError> {
    sqlx::query("DELETE FROM write_records WHERE write_id IN (SELECT write_id FROM temporal_append_intents WHERE entity_id LIKE $1)")
        .bind(like)
        .execute(&mut **transaction)
        .await
        .map_err(DatabaseResetError::Sqlx)?;
    sqlx::query("DELETE FROM temporal_append_intents WHERE entity_id LIKE $1")
        .bind(like)
        .execute(&mut **transaction)
        .await
        .map_err(DatabaseResetError::Sqlx)?;
    Ok(())
}

/// Global reset or isolated cleanup failure.
#[derive(Debug, Error)]
pub enum DatabaseResetError {
    /// The serialized-state capability was unavailable.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// `SQLx` could not execute the reset or cleanup transaction.
    #[error("test database reset failed: {0}")]
    Sqlx(#[source] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use crate::database_reset::is_deadlock;

    #[test]
    fn a_non_database_error_is_not_treated_as_a_deadlock() {
        assert!(!is_deadlock(&sqlx::Error::PoolClosed));
    }
    use crate::database_reset::{PREFIX_DELETES, RESET_TENANT_SQL};

    /// The relation list of the reset script's `TRUNCATE`, without the header
    /// comment that explains which relations are deliberately absent from it.
    fn truncated_relations() -> &'static str {
        let (_, statement) = RESET_TENANT_SQL.split_once("TRUNCATE").unwrap_or_else(|| panic!("the reset script truncates"));
        let (relations, _) = statement
            .split_once("RESTART IDENTITY")
            .unwrap_or_else(|| panic!("the truncation restarts identities"));
        relations
    }

    #[test]
    fn reset_script_covers_mutable_tables_and_preserves_cached_dictionaries() {
        for table in [
            "entities",
            "entity_documents",
            "entity_search",
            "jsonld_contexts",
            "temporal_entities",
            "temporal_attribute_streams",
            "attribute_history",
            "attribute_history_instance_claims",
            "subscriptions",
            "subscription_delivery_state",
            "context_source_registrations",
            "registration_information",
            "snapshots",
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
    fn reset_script_removes_snapshot_context_spaces_and_preserves_the_live_space() {
        assert!(RESET_TENANT_SQL.contains("DELETE FROM context_spaces WHERE kind = 'snapshot'"));
        assert!(!RESET_TENANT_SQL.contains("TRUNCATE context_spaces"));
    }

    #[test]
    fn reset_script_leaves_the_schema_contract_singleton_in_place() {
        // Truncating `schema_metadata` makes the tenant read as never-stamped,
        // and the startup verification refuses every connection after it.
        assert!(!truncated_relations().contains("schema_metadata"));
    }
}
