//! The database one rehearsal runs in: created on the server, built at a
//! released schema, seeded by the test, upgraded to this build's schema and
//! dropped.

use crate::{release_schema::v1_0_0_migrator, upgrade_database_error::UpgradeDatabaseError, upgrade_server::UpgradeServer};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_context_store::{
    tenant_database::{DatabaseDrop, create_database_if_missing, drop_database_unless_in_use},
    tenant_migration::{migrate_tenant_db, open_schema_change_pool},
};
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use andromeda_retry::{retry_policy::RetryPolicy, schedule::BackoffSchedule, tokio_retry_delay::TokioRetryDelay};
use sqlx::PgPool;
use std::{num::NonZeroU32, time::Duration};
use url::Url;
use uuid::Uuid;

/// How long a step waits for the database's one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// The wait for the server to end the session of a closed connection before a
/// drop is refused: twenty runs a quarter of a second apart.
const DROP_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(19),
    schedule: BackoffSchedule::constant(Duration::from_millis(250)),
};

/// A database created for one rehearsal, with the one connection its migrations
/// and its test share.
#[derive(Debug)]
pub struct UpgradeDatabase {
    /// The server's URL, which creating and dropping the database go through.
    server: DatabaseUrl,
    /// The database's own name, unique to this rehearsal.
    name: TenantDatabaseName,
    /// The database's own URL.
    url: Url,
    /// The one connection.
    pool: PgPool,
}

impl UpgradeDatabase {
    /// Creates an empty database of its own on `server`.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError`] when the database cannot be created or
    /// reached.
    pub async fn create(server: &UpgradeServer) -> Result<UpgradeDatabase, UpgradeDatabaseError> {
        let name = fresh_name();
        create_database_if_missing(server.url(), &name).await?;
        let url = server.url().with_database_name(&name);
        let pool = open_schema_change_pool(&url, ACQUIRE_TIMEOUT).await?;
        Ok(UpgradeDatabase {
            server: server.url().clone(),
            name,
            url,
            pool,
        })
    }

    /// Applies the tenant migrations v1.0.0 sealed, leaving the database as a
    /// v1.0.0 tenant is.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::ReleaseMigration`] when a migration
    /// fails.
    pub async fn migrate_to_v1_0_0(&self) -> Result<(), UpgradeDatabaseError> {
        v1_0_0_migrator().run(&self.pool).await?;
        Ok(())
    }

    /// Applies every migration this build embeds that the database lacks, as
    /// the broker does when it opens a tenant, then opens the connection again
    /// so no statement prepared against the earlier schema is reused.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError`] when a migration fails or the
    /// connection cannot be opened again.
    pub async fn migrate_to_current(&mut self) -> Result<(), UpgradeDatabaseError> {
        migrate_tenant_db(&self.pool).await?;
        self.pool.close().await;
        self.pool = open_schema_change_pool(&self.url, ACQUIRE_TIMEOUT).await?;
        Ok(())
    }

    /// The connection a test seeds and reads through.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Closes the connection and drops the database.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::StillConnected`] when another session
    /// holds the database, and [`UpgradeDatabaseError::Store`] when the drop
    /// fails.
    pub async fn drop_database(self) -> Result<(), UpgradeDatabaseError> {
        self.pool.close().await;
        DROP_RETRY
            .run(
                &TokioRetryDelay,
                |error, _| matches!(error, UpgradeDatabaseError::StillConnected),
                async |_| match drop_database_unless_in_use(&self.server, &self.name).await? {
                    DatabaseDrop::Dropped => Ok(()),
                    DatabaseDrop::InUse => Err(UpgradeDatabaseError::StillConnected),
                },
            )
            .await
    }
}

/// A database name no other rehearsal takes, within PostgreSQL's identifier
/// limit of 63 bytes.
fn fresh_name() -> TenantDatabaseName {
    TenantDatabaseName::from(format!("upgrade_{}", Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests {
    use crate::upgrade_database::fresh_name;

    #[test]
    fn two_rehearsals_never_share_a_database_name() {
        assert_ne!(fresh_name(), fresh_name());
    }

    #[test]
    fn a_database_name_fits_the_identifier_limit() {
        assert!(fresh_name().as_str().len() <= 63);
    }
}
