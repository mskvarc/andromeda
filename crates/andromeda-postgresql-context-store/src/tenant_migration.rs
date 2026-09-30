//! Migrating the database behind one tenant to the schema this binary embeds.
//!
//! The platform database carries the platform set and the tenant set; every
//! other tenant database carries the tenant set. Each run stamps and verifies
//! the schema contract and then brings every Snapshot's branch schemas up to
//! the branch version, quarantining a Snapshot whose upgrade fails.

use crate::{
    error::ContextStoreError,
    migration_set::{branch::BranchMigrationSet, platform::platform_migrator, tenant::tenant_migrator},
    schema_contract::{
        contract::current_tenant_schema_contract,
        error::SchemaContractError,
        schema_metadata::{ensure_schema_contract, verify_schema_contract},
        server_requirements::ExtensionPresence,
    },
    schema_ddl::database_settings::set_database_synchronous_commit,
    snapshot::branch_upgrade::{BranchUpgradeReport, upgrade_snapshot_branches},
};
use andromeda_postgresql_pool::configure_pool_options;
use andromeda_postgresql_seaquery::advisory_lock::advisory_xact_lock;
use sea_query::{Expr, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Executor, PgPool, error::DatabaseError};
use std::time::Duration;
use url::Url;

/// Install the broker's durability on the platform database.
///
/// Every other tenant database receives it when
/// [`create_database_if_missing`](crate::tenant_database::create_database_if_missing)
/// creates it; the platform database is created by the operator, so the broker
/// installs the setting when it connects. `ALTER DATABASE ... SET` makes every
/// later request-path session -- including one a transaction-pooling proxy
/// opens -- inherit it without a per-connection `SET`.
///
/// Several brokers start against one platform database at once, and PostgreSQL
/// does not let two sessions write the same `pg_db_role_setting` row
/// concurrently: the loser fails with a unique violation on the first insert or
/// "tuple concurrently updated" afterwards. The statement therefore runs in a
/// transaction that first takes an advisory lock keyed on the database, so the
/// brokers apply it one after another.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the transaction, the lock or
/// the `ALTER DATABASE` fails.
pub async fn configure_platform_database(pool: &PgPool, platform_database_name: &str, synchronous_commit_setting: &str) -> Result<(), ContextStoreError> {
    let mut transaction = pool.begin().await?;
    let (lock_sql, lock_values) = platform_configuration_lock(platform_database_name).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(lock_sql.as_str()), lock_values)
        .execute(&mut *transaction)
        .await?;
    let alter = set_database_synchronous_commit(platform_database_name, synchronous_commit_setting);
    transaction.execute(sqlx::AssertSqlSafe(alter)).await?;
    transaction.commit().await?;
    Ok(())
}

/// The transaction advisory lock that serializes configuring one platform database.
fn platform_configuration_lock(platform_database_name: &str) -> SelectStatement {
    Query::select()
        .expr(advisory_xact_lock(Expr::val(format!(
            "andromeda:platform-configuration:{platform_database_name}"
        ))))
        .to_owned()
}

/// Run the platform and tenant migrations against the platform database, in
/// that order.
///
/// The pool must reach PostgreSQL directly, never through a transaction-pooling
/// proxy: the migrator holds a session advisory lock across the run, which a
/// per-transaction backend assignment would break.
///
/// The platform database is also the unnamed default tenant's database, so it
/// carries the tenant schema and its contract beside the platform tables. The
/// platform set runs first because it holds the lowest versions of the one
/// version space every migration directory shares; the two sets are recorded
/// in separate ledgers ([`crate::migration_set`]), so neither sees the other's
/// history.
///
/// # Errors
///
/// Returns [`ContextStoreError::Migration`] when a platform migration fails,
/// and every failure [`migrate_tenant_db`] returns.
pub async fn migrate_platform_db(pool: &PgPool) -> Result<(), ContextStoreError> {
    platform_migrator().run(pool).await?;
    migrate_tenant_db(pool).await
}

/// Run the tenant migrations against the given pool, stamp and verify the
/// schema contract the migrated database now carries, and bring every
/// Snapshot's branch schemas up to the branch version this binary embeds.
///
/// The stamp is written here rather than by a migration because a migration
/// cannot digest the migration set it belongs to. Verification refuses a tenant
/// whose schema, server or extensions are not what this binary was built
/// against, before any caller can publish the pool — except a tenant whose
/// stamp is behind this binary's contract and whose applied migration history
/// proves it, which [`ensure_schema_contract`] re-stamps in place: a fleet
/// upgrade migrates every tenant's schema on this same call, and a stamp left
/// at the previous binary's version must not then read back as drift.
///
/// A Snapshot whose branch upgrade fails is quarantined rather than failing the
/// tenant: the tenant and every other Snapshot keep serving.
///
/// # Errors
///
/// Returns [`ContextStoreError::Migration`] when a tenant migration fails,
/// [`ContextStoreError::SchemaContract`] when the embedded migration set has no
/// contract or the migrated database does not carry the schema, server or
/// extensions this binary requires, and [`ContextStoreError::Database`] or
/// [`ContextStoreError::BranchVersion`] when the Snapshot branches cannot be
/// listed or an upgrade outcome cannot be recorded.
pub async fn migrate_tenant_db(pool: &PgPool) -> Result<(), ContextStoreError> {
    tenant_migrator().run(pool).await?;

    let contract = current_tenant_schema_contract()?;
    ensure_schema_contract(pool, &contract).await?;
    report_branch_upgrade(upgrade_snapshot_branches(pool, &BranchMigrationSet::embedded()).await?);
    Ok(())
}

/// Bring every Snapshot's branch schemas in one tenant database up to the
/// branch version this binary embeds, over a direct one-connection pool.
///
/// Called on a tenant load whose schema is already current but whose
/// [`snapshot_branch_state`](crate::snapshot::branch_upgrade::snapshot_branch_state) probe found a Snapshot behind -- a binary that
/// appended a branch migration started against a tenant another replica had
/// already migrated, or an earlier upgrade quarantined a Snapshot.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the pool cannot connect, and
/// the errors [`upgrade_snapshot_branches`] returns.
pub async fn upgrade_single_tenant_snapshot_branches(database_url: &Url, acquire_timeout: Duration) -> Result<(), ContextStoreError> {
    let pool = configure_pool_options(1, acquire_timeout).connect(database_url.as_str()).await?;
    let outcome = upgrade_snapshot_branches(&pool, &BranchMigrationSet::embedded()).await;
    pool.close().await;
    report_branch_upgrade(outcome?);
    Ok(())
}

/// Log what an upgrade run did, when it did anything.
fn report_branch_upgrade(report: BranchUpgradeReport) {
    if report.upgraded > 0 || report.quarantined > 0 {
        tracing::info!(
            upgraded = report.upgraded,
            quarantined = report.quarantined,
            event = "snapshot_branches_upgraded",
            "Snapshot branch schemas upgraded"
        );
    }
}

/// Connect to a single tenant database over a one-connection pool, migrate it,
/// and close the pool.
///
/// `database_url` must reach PostgreSQL directly, never a transaction-pooling
/// proxy: the migrator holds a session advisory lock across the run, which a
/// per-transaction backend assignment would break. Migration is single-threaded,
/// so the pool holds exactly one connection.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the pool cannot connect, and
/// the errors [`migrate_tenant_db`] returns when the migration or the schema
/// contract check fails.
pub async fn migrate_single_tenant_db(database_url: &Url, acquire_timeout: Duration) -> Result<(), ContextStoreError> {
    let pool = configure_pool_options(1, acquire_timeout).connect(database_url.as_str()).await?;
    migrate_tenant_db(&pool).await?;
    pool.close().await;
    Ok(())
}

/// Whether a tenant's schema is the one this binary was built against, or must
/// be migrated up to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantSchemaState {
    /// The schema matches the binary's contract; no migration is needed and the
    /// caller may serve over the pool it already opened.
    Current,
    /// The schema is absent or behind and must be migrated before serving.
    NeedsMigration,
}

/// Decide whether a tenant's schema is current or must be migrated, without
/// running the migrator.
///
/// Built on the read-only [`verify_schema_contract`], which issues only single
/// statements and so is safe over a transaction pooler. A schema that is simply
/// not there yet -- unstamped, a version behind, its `schema_metadata` relation
/// undefined, or its versioned extensions not yet installed -- reads as
/// [`TenantSchemaState::NeedsMigration`], so the caller runs the migrator over a
/// direct connection. Genuine drift the migrator cannot resolve -- a digest
/// mismatch at the same version, a stamp ahead of the contract, or a server or
/// installed extension below the floor -- is returned as the refusal it is,
/// rather than migrated over.
///
/// # Errors
///
/// Returns [`ContextStoreError::SchemaContract`] for the drift cases above, and
/// when the embedded migration set has no contract or the read itself fails for
/// a reason other than an undefined relation.
pub async fn tenant_schema_state(pool: &PgPool) -> Result<TenantSchemaState, ContextStoreError> {
    let contract = current_tenant_schema_contract()?;
    match verify_schema_contract(pool, &contract).await {
        Ok(()) => Ok(TenantSchemaState::Current),
        Err(error) if schema_error_is_migratable(&error) => Ok(TenantSchemaState::NeedsMigration),
        Err(error) => Err(error.into()),
    }
}

/// Whether a schema-contract refusal is one the migrator resolves by bringing
/// the schema up to the binary's contract.
///
/// True for a schema that is not there yet; false for drift the migrator cannot
/// fix, which stays a refusal. See [`tenant_schema_state`].
fn schema_error_is_migratable(error: &SchemaContractError) -> bool {
    match error {
        SchemaContractError::Unstamped { .. } => true,
        SchemaContractError::SchemaVersionMismatch { expected, found } => found < expected,
        SchemaContractError::Database(source) => code_is_undefined_relation(source.as_database_error().and_then(DatabaseError::code).as_deref()),
        SchemaContractError::PostGisBelowMinimum { found, .. } | SchemaContractError::TimescaleDbBelowMinimum { found, .. } => {
            matches!(found, ExtensionPresence::Absent)
        }
        SchemaContractError::MigrationsDigestMismatch { .. }
        | SchemaContractError::PostgresBelowMinimum { .. }
        | SchemaContractError::ServerEncodingNotUtf8 { .. }
        | SchemaContractError::ServerEncodingUnreadable(_)
        | SchemaContractError::NoMigrations
        | SchemaContractError::SchemaVersionValue(_)
        | SchemaContractError::MigrationsDigestValue(_)
        | SchemaContractError::ExtensionVersionValue(_) => false,
    }
}

/// Whether a SQLSTATE names an undefined relation (`42P01`), which is how an
/// unmigrated tenant's absent `schema_metadata` table surfaces on the read.
fn code_is_undefined_relation(code: Option<&str>) -> bool {
    code == Some("42P01")
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_contract::{
            error::SchemaContractError,
            migrations_digest::MigrationsDigest,
            schema_version::SchemaVersion,
            server_requirements::{ExtensionPresence, ExtensionVersion, REQUIRED_POSTGIS_VERSION},
        },
        tenant_migration::{code_is_undefined_relation, platform_configuration_lock, schema_error_is_migratable},
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::PostgresQueryBuilder;

    /// Every broker configuring one platform database takes the same
    /// transaction lock, and brokers configuring different ones do not contend.
    #[test]
    fn configuring_a_platform_database_takes_a_transaction_lock_keyed_on_it() {
        let (sql, values) = platform_configuration_lock("andromeda").build(PostgresQueryBuilder);
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values, platform_configuration_lock("andromeda").build(PostgresQueryBuilder).1);
        assert_ne!(values, platform_configuration_lock("other").build(PostgresQueryBuilder).1);
    }

    /// The contention a burst of tenant provisioning meets is retried: 55006
    /// (template in use), 40001, 40P01 and 53300. Every other failure, and the
    /// absence of a SQLSTATE, is returned at once.
    /// A create that lost the race to a concurrent create of the same database
    /// name is a success, not a failure, so 42P04 (and only 42P04) is a duplicate.
    /// A version behind the contract is a completed-migration-away, so the cold
    /// path migrates; a version ahead is a downgrade the migrator cannot make,
    /// so it stays a refusal.
    #[test]
    fn a_stamp_behind_the_contract_needs_migration() {
        let expected = SchemaVersion::try_from(5).expect("a valid schema version");
        let behind = SchemaVersion::try_from(4).expect("a valid schema version");
        assert!(schema_error_is_migratable(&SchemaContractError::SchemaVersionMismatch {
            expected,
            found: behind
        }));

        let ahead = SchemaVersion::try_from(6).expect("a valid schema version");
        assert!(!schema_error_is_migratable(&SchemaContractError::SchemaVersionMismatch {
            expected,
            found: ahead
        }));
    }

    /// A digest mismatch at the same version means the fleet runs two binaries,
    /// which running the migrator cannot resolve: it is refused, not migrated.
    #[test]
    fn a_digest_mismatch_at_the_same_version_is_refused_not_migrated() {
        let expected = MigrationsDigest::try_from("a".repeat(64).as_str()).expect("a valid digest");
        let found = MigrationsDigest::try_from("b".repeat(64).as_str()).expect("a valid digest");
        assert!(!schema_error_is_migratable(&SchemaContractError::MigrationsDigestMismatch { expected, found }));
    }

    /// The absent `schema_metadata` relation an unmigrated tenant carries reads
    /// back as SQLSTATE 42P01, which routes to a migration; no other SQLSTATE
    /// does.
    #[test]
    fn an_undefined_schema_metadata_relation_needs_migration() {
        assert!(code_is_undefined_relation(Some("42P01")));
        for other in [Some("42703"), Some("08006"), Some("23505"), None] {
            assert!(!code_is_undefined_relation(other), "{other:?} is not an undefined relation");
        }
    }

    /// An extension a migration installs (absent) needs migrating; one that is
    /// installed but below the floor is a server upgrade and stays a refusal.
    #[test]
    fn an_absent_extension_needs_migration_but_one_below_the_floor_is_refused() {
        let absent = SchemaContractError::PostGisBelowMinimum {
            required: REQUIRED_POSTGIS_VERSION,
            found: ExtensionPresence::Absent,
        };
        assert!(schema_error_is_migratable(&absent), "a migration installs an absent extension");

        let too_old = SchemaContractError::PostGisBelowMinimum {
            required: REQUIRED_POSTGIS_VERSION,
            found: ExtensionPresence::Installed(ExtensionVersion::new(2, 5)),
        };
        assert!(!schema_error_is_migratable(&too_old), "an installed-but-too-old extension is a server upgrade");
    }
}
