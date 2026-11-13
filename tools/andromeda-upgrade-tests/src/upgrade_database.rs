//! The database one rehearsal runs in: created on the server, built at a
//! released schema, seeded by the test, upgraded to this build's schema and
//! dropped.

use crate::{release_schema::V1_0_0_LAST_MIGRATION, upgrade_database_error::UpgradeDatabaseError, upgrade_server::UpgradeServer};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_branch::{snapshot_schema_name, snapshot_search_path};
use andromeda_postgresql_context_store::{
    migration_set::{branch::branch_migrator, migration_version::MigrationVersion, platform::platform_migrator, tenant::tenant_migrator},
    tenant_database::create_database_if_missing,
    tenant_migration::{migrate_platform_db, migrate_tenant_db, open_schema_change_pool},
};
use andromeda_postgresql_pool::{database_url::DatabaseUrl, pool_options::configure_pool_options, schema_change_lock::schema_change_connect_options};
use sqlx::PgPool;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

/// How long a step waits for the database's one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// A database created for one rehearsal, with the one connection its migrations
/// and its test share in `public`. A Snapshot branch schema is reached through
/// a pool of its own.
#[derive(Debug)]
pub struct UpgradeDatabase {
    /// The server, which creating and dropping the database go through.
    server: UpgradeServer,
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
            server: server.clone(),
            name,
            url,
            pool,
        })
    }

    /// Applies the tenant migrations v1.0.0 sealed, leaving `public` as a
    /// v1.0.0 tenant holds it.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::ReleaseMigration`] when a migration
    /// fails.
    pub async fn migrate_to_v1_0_0(&self) -> Result<(), UpgradeDatabaseError> {
        self.migrate_public_through(V1_0_0_LAST_MIGRATION).await
    }

    /// Applies the tenant migrations through `through` in `public` alone,
    /// leaving every Snapshot branch schema where it is, as an upgrade cut
    /// short after `public` applied them leaves the database.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::ReleaseMigration`] when a migration
    /// fails.
    pub async fn migrate_public_through(&self, through: MigrationVersion) -> Result<(), UpgradeDatabaseError> {
        tenant_migrator().run_to(through.get(), &self.pool).await?;
        Ok(())
    }

    /// Creates the branch schema `branch` of the Snapshot `key` and applies
    /// the branch migrations through `through` in it, as the broker's branch
    /// migrator does: the schema first on the search path, recorded in the
    /// schema's own ledger.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::Connection`] when the schema cannot be
    /// reached, and [`UpgradeDatabaseError::ReleaseMigration`] when a
    /// migration fails.
    pub async fn build_branch_schema(&self, key: SnapshotKey, branch: SnapshotBranch, through: MigrationVersion) -> Result<(), UpgradeDatabaseError> {
        let pool = self.branch_pool(key, branch).await?;
        let mut migrator = branch_migrator(key, branch);
        migrator.create_schema(snapshot_schema_name(key, branch));
        let built = migrator.run_to(through.get(), &pool).await;
        pool.close().await;
        Ok(built?)
    }

    /// A pool whose one connection resolves an unqualified relation in the
    /// branch schema `branch` of the Snapshot `key` first, and a tenant-global
    /// one in `public`, as the broker's statements on that branch do. The test
    /// closes it before it drops the database.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::Connection`] when the database cannot
    /// be reached.
    pub async fn branch_pool(&self, key: SnapshotKey, branch: SnapshotBranch) -> Result<PgPool, UpgradeDatabaseError> {
        let options = schema_change_connect_options(self.url.as_str())?.options([("search_path", snapshot_search_path(key, branch))]);
        Ok(configure_pool_options(1, ACQUIRE_TIMEOUT).connect_with(options).await?)
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
        self.reconnect().await
    }

    /// Applies the platform migrations v1.0.0 sealed, then the tenant
    /// migrations it sealed, leaving the database as v1.0.0 holds the platform
    /// database, which is also the default tenant's.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError::ReleaseMigration`] when a migration
    /// fails.
    pub async fn migrate_platform_to_v1_0_0(&self) -> Result<(), UpgradeDatabaseError> {
        platform_migrator().run_to(V1_0_0_LAST_MIGRATION.get(), &self.pool).await?;
        self.migrate_to_v1_0_0().await
    }

    /// Applies every migration this build embeds that the platform database
    /// lacks, as the broker does when it starts, then opens the connection
    /// again so no statement prepared against the earlier schema is reused.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeDatabaseError`] when a migration fails or the
    /// connection cannot be opened again.
    pub async fn migrate_platform_to_current(&mut self) -> Result<(), UpgradeDatabaseError> {
        migrate_platform_db(&self.pool).await?;
        self.reconnect().await
    }

    /// Closes the connection and opens a new one.
    async fn reconnect(&mut self) -> Result<(), UpgradeDatabaseError> {
        self.pool.close().await;
        self.pool = open_schema_change_pool(&self.url, ACQUIRE_TIMEOUT).await?;
        Ok(())
    }

    /// The connection a test seeds and reads through.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The database's own URL, for a broker component the test opens on it.
    #[must_use]
    pub fn database_url(&self) -> DatabaseUrl {
        DatabaseUrl::new(self.url.clone())
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
        self.server.drop_database(&self.name).await
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
