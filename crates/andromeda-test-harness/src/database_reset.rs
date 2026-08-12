//! Authoritative global reset and complete prefix-owned cleanup.

use crate::{database_isolation::SerializedLease, identifiers::TestIdentifiers};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use thiserror::Error;

/// Authoritative tenant reset SQL maintained alongside the test harness.
pub const RESET_TENANT_SQL: &str = include_str!("../sql/reset-tenant.sql");

/// Reset every mutable tenant table while preserving the cached `attribute_names` dictionary, the `schema_metadata` contract row, and the seeded unit-code catalogue.
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
pub async fn reset_owned_tenant(pool: &PgPool) -> Result<(), DatabaseResetError> {
    pool.execute(RESET_TENANT_SQL).await.map_err(DatabaseResetError::Sqlx)?;
    Ok(())
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
const PREFIX_DELETES: &[&str] = &[
    "DELETE FROM subscriptions WHERE id LIKE $1",
    "DELETE FROM context_source_registrations WHERE id LIKE $1",
    "DELETE FROM snapshots WHERE id LIKE $1",
    "DELETE FROM entities WHERE id LIKE $1",
    "DELETE FROM attribute_history WHERE stream_id IN (SELECT s.id FROM temporal_attribute_streams s \
     JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key WHERE te.id LIKE $1)",
    "DELETE FROM scope_history WHERE temporal_entity_key IN (SELECT entity_key FROM temporal_entities WHERE id LIKE $1)",
    "DELETE FROM temporal_entities WHERE id LIKE $1",
];

/// Delete every row discoverably owned by one isolated identifier prefix.
pub async fn cleanup_prefix(pool: &PgPool, identifiers: &TestIdentifiers) -> Result<(), DatabaseResetError> {
    let like = format!("{}%", identifiers.prefix());
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::Sqlx)?;
    delete_temporal_intents(&mut transaction, &like).await?;
    for statement in PREFIX_DELETES {
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
    Isolation(#[from] crate::database_isolation::DatabaseIsolationError),
    /// SQLx could not execute the reset or cleanup transaction.
    #[error("test database reset failed: {0}")]
    Sqlx(#[source] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use crate::database_reset::{PREFIX_DELETES, RESET_TENANT_SQL};

    /// The relation list of the reset script's `TRUNCATE`, without the header
    /// comment that explains which relations are deliberately absent from it.
    fn truncated_relations() -> &'static str {
        let (_, statement) = RESET_TENANT_SQL
            .split_once("TRUNCATE")
            .unwrap_or_else(|| unreachable!("the reset script truncates"));
        let (relations, _) = statement
            .split_once("RESTART IDENTITY")
            .unwrap_or_else(|| unreachable!("the truncation restarts identities"));
        relations
    }

    #[test]
    fn reset_script_covers_mutable_tables_and_preserves_cached_dictionary() {
        for table in [
            "entities",
            "jsonld_contexts",
            "temporal_entities",
            "temporal_attribute_streams",
            "attribute_history",
            "subscriptions",
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
            .unwrap_or_else(|| unreachable!("the catalogue is deleted"));
        for relation in ["DELETE FROM attribute_history", "DELETE FROM scope_history"] {
            let dependent = PREFIX_DELETES
                .iter()
                .position(|statement| statement.starts_with(relation))
                .unwrap_or_else(|| unreachable!("{relation} is deleted"));
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
    fn reset_script_leaves_the_schema_contract_singleton_in_place() {
        // Truncating `schema_metadata` makes the tenant read as never-stamped,
        // and the startup verification refuses every connection after it.
        assert!(!truncated_relations().contains("schema_metadata"));
    }
}
