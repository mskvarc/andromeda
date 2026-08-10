//! Authoritative global reset and complete prefix-owned cleanup.

use crate::{database_isolation::SerializedLease, identifiers::TestIdentifiers};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use thiserror::Error;

/// Authoritative tenant reset SQL maintained alongside the test harness.
pub const RESET_TENANT_SQL: &str = include_str!("../sql/reset-tenant.sql");

/// Reset every mutable tenant table while preserving the cached `attribute_names` dictionary.
pub async fn reset_tenant(lease: &mut SerializedLease) -> Result<(), DatabaseResetError> {
    let connection = lease.connection()?;
    connection.execute(RESET_TENANT_SQL).await.map_err(DatabaseResetError::Sqlx)?;
    Ok(())
}

/// Delete every row discoverably owned by one isolated identifier prefix.
pub async fn cleanup_prefix(pool: &PgPool, identifiers: &TestIdentifiers) -> Result<(), DatabaseResetError> {
    let like = format!("{}%", identifiers.prefix());
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::Sqlx)?;
    delete_temporal_intents(&mut transaction, &like).await?;
    for statement in [
        "DELETE FROM subscriptions WHERE id LIKE $1",
        "DELETE FROM context_source_registrations WHERE id LIKE $1",
        "DELETE FROM snapshots WHERE id LIKE $1",
        "DELETE FROM entities WHERE id LIKE $1",
    ] {
        sqlx::query(statement)
            .bind(&like)
            .execute(&mut *transaction)
            .await
            .map_err(DatabaseResetError::Sqlx)?;
    }
    transaction.commit().await.map_err(DatabaseResetError::Sqlx)?;
    Ok(())
}

async fn delete_temporal_intents(transaction: &mut Transaction<'_, Postgres>, like: &str) -> Result<(), DatabaseResetError> {
    sqlx::query("DELETE FROM temporal_history_sink_receipts WHERE intent_id IN (SELECT intent_id FROM temporal_append_intents WHERE entity_id LIKE $1)")
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
    Isolation(#[from] crate::database_isolation::DatabaseIsolationError),
    /// SQLx could not execute the reset or cleanup transaction.
    #[error("test database reset failed: {0}")]
    Sqlx(#[source] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use crate::database_reset::RESET_TENANT_SQL;

    #[test]
    fn reset_script_covers_mutable_tables_and_preserves_cached_dictionary() {
        for table in [
            "entities",
            "jsonld_contexts",
            "temporal_entity_attributes",
            "attribute_history",
            "subscriptions",
            "context_source_registrations",
            "snapshots",
            "scope_history",
            "notification_outbox",
            "attribute_current",
            "pending_side_effects",
            "temporal_append_intents",
            "temporal_history_sink_receipts",
        ] {
            assert!(RESET_TENANT_SQL.contains(table), "reset SQL must cover {table}");
        }
        assert!(!RESET_TENANT_SQL.contains("attribute_names"));
    }
}
