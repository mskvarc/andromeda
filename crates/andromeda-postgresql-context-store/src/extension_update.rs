//! Bringing a tenant database's versioned extensions up to the versions the
//! server's installation ships.
//!
//! An image upgrade installs a newer `TimescaleDB` and `PostGIS`, but every
//! database keeps the version its extension was created or last updated at
//! until `ALTER EXTENSION ... UPDATE` runs in it. `TimescaleDB` adds two
//! constraints: the update is refused in a session that has already loaded the
//! old library, and once it commits, every other session that loaded the old
//! library is closed by the extension at its next statement. The update
//! therefore runs as the first statement of a connection of its own, and every
//! read that decides whether an update is due keeps `TimescaleDB` unloaded, so
//! no connection that outlives the update ever carried the old library.
//!
//! The same reconciliation installs every [`OptionalExtension`] the server ships
//! and the database lacks, when the connecting role may create it. It runs
//! before a database is migrated and whenever a tenant's probe finds one due, so
//! a fresh database receives it before its first migration and a database
//! created before the server shipped it receives it the next time the broker
//! opens it.
//!
//! Brokers starting together serialize on a transaction advisory lock keyed on
//! the database and read the versions again under it, so one of them updates
//! and the others find nothing left to do. Every connection here waits a
//! bounded time for each lock, and an update refused a lock runs again.

use crate::{
    error::ContextStoreError,
    optional_extension::OptionalExtension,
    schema_contract::{
        error::SchemaContractError,
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
};
use andromeda_postgresql_pool::{
    schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, is_lock_not_available, schema_change_connect_options},
    sql_state::SqlState,
};
use andromeda_postgresql_seaquery::{
    advisory_lock::advisory_xact_lock,
    command::Command,
    extension_creation::ExtensionCreation,
    extension_update::ExtensionUpdate,
    introspection::{current_role_is_superuser, extension_shipped_version, extension_trusted, extension_version},
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
};
use andromeda_retry::tokio_retry_delay::TokioRetryDelay;
use sea_query::{Alias, Expr, ExprTrait, Func, Query, SelectStatement, SimpleExpr};
use sqlx::{
    Acquire,
    Connection,
    PgConnection,
    Postgres,
    Row,
    Transaction,
    postgres::{PgConnectOptions, PgRow},
};
use strum::IntoEnumIterator;
use url::Url;

/// One versioned extension installed below the version the server ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingExtensionUpdate {
    /// The extension to update.
    pub extension: VersionedExtension,
    /// The version `pg_extension` records for this database.
    pub installed: ExtensionVersion,
    /// The version the server's installation ships, which the update reaches.
    pub shipped: ExtensionVersion,
}

/// Open a transaction in which `TimescaleDB` stays unloaded.
///
/// Every statement of the transaction may read the catalogues and the
/// broker's own tables; none may call a `TimescaleDB` function, which is not
/// loaded. The connection loads the extension again at its first statement
/// after the transaction ends, at whatever version is installed by then.
///
/// # Errors
///
/// Returns the driver error when the transaction cannot begin or the setting
/// cannot be applied, including a pool that cannot hand out a connection in
/// time.
pub async fn begin_without_timescaledb<'c, A>(connection: A) -> Result<Transaction<'c, Postgres>, sqlx::Error>
where
    A: Acquire<'c, Database = Postgres>,
{
    let mut transaction = connection.begin().await?;
    // A `SET`, not `set_config`: the `TimescaleDB` loader loads the extension
    // library while it analyses any other statement, a `SELECT set_config(...)`
    // included, before that statement could turn loading off. `LOCAL` scopes
    // it to this transaction, which a transaction-pooling proxy keeps on one
    // backend and which hands the backend back with loading restored.
    SettingAssignment::new(Setting::TimescaledbDisableLoad, "on", SettingScope::Transaction)
        .to_query()
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

/// The versioned extensions this database holds below the version the server
/// ships.
///
/// An extension the database does not hold is not pending: the migrations
/// create it at the shipped version. One the installation no longer ships is
/// not pending either, since nothing could update it.
///
/// Run it inside [`begin_without_timescaledb`], so the read leaves its
/// connection usable after an update.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails and
/// [`ContextStoreError::SchemaContract`] when a reported version cannot be
/// read.
pub async fn pending_extension_updates(connection: &mut PgConnection) -> Result<Vec<PendingExtensionUpdate>, ContextStoreError> {
    let row = extension_versions_statement().to_query().fetch_one(connection).await?;
    VersionedExtension::iter()
        .filter_map(|extension| {
            read_versions(&row, extension)
                .map(|(installed, shipped)| pending_update(extension, installed, shipped))
                .transpose()
        })
        .collect()
}

/// The optional extensions the server ships, this database lacks, and the
/// session's role may create: as a superuser, or because the shipped version is
/// trusted.
///
/// An extension the role may not create is not due, so a database whose role
/// cannot install it does not read as needing reconciliation on every open.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails or a column
/// does not decode.
pub async fn installable_optional_extensions(connection: &mut PgConnection) -> Result<Vec<OptionalExtension>, ContextStoreError> {
    let row = installable_extensions_statement().to_query().fetch_one(connection).await?;
    OptionalExtension::iter()
        .map(|extension| -> Result<Option<OptionalExtension>, ContextStoreError> {
            Ok(row.try_get::<bool, _>(extension.installable_column())?.then_some(extension))
        })
        .filter_map(Result::transpose)
        .collect()
}

/// Update every versioned extension of the database `database_url` names to
/// the version the server ships, and install every optional extension
/// [`installable_optional_extensions`] finds due.
///
/// Run before anything else touches the database on this start, so no
/// connection the broker keeps has loaded the old `TimescaleDB` library: the
/// lock and the version read run without loading it, and each update runs on a
/// connection opened for it alone. An update the connecting role may not make
/// is logged and skipped, leaving the schema contract's floor to decide whether
/// the installed version still serves; an installation refused the same way is
/// logged and skipped, leaving the feature that uses the extension to report
/// its absence.
///
/// # Errors
///
/// Returns [`ContextStoreError::DatabaseUrlUnnamed`] when the URL names no
/// database, [`ContextStoreError::Database`] when a connection cannot be
/// opened, the lock or the read fails, or an update fails for a reason other
/// than privilege or an installation fails the same way, and
/// [`ContextStoreError::SchemaContract`] when a reported version cannot be
/// read.
pub async fn update_extensions(database_url: &Url) -> Result<(), ContextStoreError> {
    let options = schema_change_connect_options(database_url.as_str())?;
    let mut lock_holder = PgConnection::connect_with(&options).await?;
    let outcome = update_under_lock(&mut lock_holder, &options).await;
    lock_holder.close().await?;
    outcome
}

/// Take the database's update lock, read what is pending and what is due to be
/// installed, and apply both, all in one transaction that keeps `TimescaleDB`
/// unloaded on the lock holder.
async fn update_under_lock(lock_holder: &mut PgConnection, options: &PgConnectOptions) -> Result<(), ContextStoreError> {
    let database = options.get_database().ok_or(ContextStoreError::DatabaseUrlUnnamed)?;
    let mut transaction = begin_without_timescaledb(&mut *lock_holder).await?;
    extension_update_lock(database).to_query().execute(&mut *transaction).await?;
    for pending in pending_extension_updates(&mut transaction).await? {
        apply_update(options, pending).await?;
    }
    for extension in installable_optional_extensions(&mut transaction).await? {
        apply_installation(options, extension).await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Run `command` as the first statement of a connection opened for it, opening
/// another and running it again while it is refused a lock.
async fn run_on_own_connection(options: &PgConnectOptions, command: &impl Command) -> Result<(), sqlx::Error> {
    SCHEMA_CHANGE_LOCK_RETRY
        .run(
            &TokioRetryDelay,
            |error, _| is_lock_not_available(error),
            |_| async move {
                let mut connection = PgConnection::connect_with(options).await?;
                let outcome = command.to_query().execute(&mut connection).await;
                connection.close().await?;
                outcome.map(drop)
            },
        )
        .await
}

/// Install one optional extension on a connection opened for it.
async fn apply_installation(options: &PgConnectOptions, extension: OptionalExtension) -> Result<(), ContextStoreError> {
    match run_on_own_connection(options, &ExtensionCreation::new(extension)).await {
        Ok(()) => {
            tracing::info!(extension = %extension, event = "extension_installed", "optional extension installed");
            Ok(())
        }
        Err(error) if is_insufficient_privilege(&error) => {
            tracing::warn!(
                extension = %extension,
                error = %error,
                event = "extension_install_refused",
                "optional extension installation needs a role that may create it; the extension stays absent"
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

/// Run one update as the first statement of a connection opened for it.
async fn apply_update(options: &PgConnectOptions, pending: PendingExtensionUpdate) -> Result<(), ContextStoreError> {
    match run_on_own_connection(options, &ExtensionUpdate::new(pending.extension)).await {
        Ok(()) => {
            tracing::info!(
                extension = %pending.extension,
                from = %pending.installed,
                to = %pending.shipped,
                event = "extension_updated",
                "extension updated to the version the server ships"
            );
            Ok(())
        }
        Err(error) if is_insufficient_privilege(&error) => {
            tracing::warn!(
                extension = %pending.extension,
                installed = %pending.installed,
                shipped = %pending.shipped,
                error = %error,
                event = "extension_update_refused",
                "extension update needs the extension's owner; the installed version is kept"
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

/// Whether a failed update was refused for privilege rather than failing: the
/// connecting role does not own the extension, or may not run an untrusted
/// extension's update script.
fn is_insufficient_privilege(error: &sqlx::Error) -> bool {
    SqlState::of(error) == Some(SqlState::InsufficientPrivilege)
}

/// One extension's installed version and the version the server ships, the
/// latter `None` when the installation does not ship it.
fn read_versions(row: &PgRow, extension: VersionedExtension) -> Result<(ExtensionPresence, Option<ExtensionVersion>), ContextStoreError> {
    let installed = ExtensionPresence::try_from(row.try_get::<Option<String>, _>(extension.installed_column())?).map_err(SchemaContractError::from)?;
    let shipped = row
        .try_get::<Option<String>, _>(extension.shipped_column())?
        .map(|raw| ExtensionVersion::try_from(raw.as_str()))
        .transpose()
        .map_err(SchemaContractError::from)?;
    Ok((installed, shipped))
}

/// The update one extension needs, or `None` when it is absent, not shipped,
/// or installed at or above the shipped version.
///
/// An installed version above the shipped one is a server running an older
/// image than the one that last updated the database; no update reaches back,
/// and the schema contract's floor decides whether it still serves.
fn pending_update(extension: VersionedExtension, installed: ExtensionPresence, shipped: Option<ExtensionVersion>) -> Option<PendingExtensionUpdate> {
    match (installed, shipped) {
        (ExtensionPresence::Installed(installed), Some(shipped)) if installed < shipped => Some(PendingExtensionUpdate { extension, installed, shipped }),
        (ExtensionPresence::Installed(_) | ExtensionPresence::Absent, Some(_) | None) => None,
    }
}

/// Read every versioned extension's installed and shipped version in one row.
fn extension_versions_statement() -> SelectStatement {
    VersionedExtension::iter().fold(Query::select(), |mut select, extension| {
        select
            .expr_as(extension_version(extension.catalog_name()), Alias::new(extension.installed_column()))
            .expr_as(extension_shipped_version(extension.catalog_name()), Alias::new(extension.shipped_column()));
        select
    })
}

/// Read, for every optional extension, whether it is due to be installed:
/// shipped, absent, and creatable by the session's role.
fn installable_extensions_statement() -> SelectStatement {
    OptionalExtension::iter().fold(Query::select(), |mut select, extension| {
        select.expr_as(installable(extension.catalog_name()), Alias::new(extension.installable_column()));
        select
    })
}

/// Whether the extension `name` is shipped, absent, and creatable by the
/// session's role; never `NULL`, since an unshipped extension has no trusted
/// flag to read.
fn installable(name: &str) -> SimpleExpr {
    let creatable = current_role_is_superuser().or(Func::coalesce([extension_trusted(name), Expr::val(false)]));
    extension_shipped_version(name)
        .is_not_null()
        .and(extension_version(name).is_null())
        .and(creatable)
}

/// The transaction advisory lock that serializes updating one database's
/// extensions.
fn extension_update_lock(database: &str) -> SelectStatement {
    Query::select()
        .expr(advisory_xact_lock(Expr::val(format!("andromeda:extension-update:{database}"))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        extension_update::{
            PendingExtensionUpdate,
            extension_update_lock,
            extension_versions_statement,
            installable_extensions_statement,
            is_insufficient_privilege,
            pending_update,
        },
        schema_contract::{
            server_requirements::{ExtensionPresence, ExtensionVersion},
            versioned_extension::VersionedExtension,
        },
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_version_read_takes_each_extensions_installed_and_shipped_version() {
        let sql = extension_versions_statement().to_string(PostgresQueryBuilder);
        for column in [
            r#"AS "postgis_version""#,
            r#"AS "postgis_shipped_version""#,
            r#"AS "timescaledb_version""#,
            r#"AS "timescaledb_shipped_version""#,
        ] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
        assert!(sql.contains(r#"FROM "pg_extension" WHERE "extname" = 'timescaledb'"#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_available_extensions" WHERE "name" = 'postgis'"#), "{sql}");
    }

    #[test]
    fn an_optional_extension_is_due_only_when_shipped_absent_and_creatable_by_the_role() {
        let sql = installable_extensions_statement().to_string(PostgresQueryBuilder);
        for clause in [
            r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'pgstattuple') IS NOT NULL"#,
            r#"(SELECT "extversion" FROM "pg_extension" WHERE "extname" = 'pgstattuple') IS NULL"#,
            r#"(SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user))"#,
            r#"COALESCE((SELECT "trusted" FROM "pg_available_extension_versions" WHERE "name" = 'pgstattuple'"#,
            r#"AS "pgstattuple_installable""#,
        ] {
            assert!(sql.contains(clause), "{clause} missing: {sql}");
        }
    }

    #[test]
    fn brokers_updating_one_database_take_the_same_lock_and_other_databases_do_not_contend() {
        let (sql, values) = extension_update_lock("andromeda").build(PostgresQueryBuilder);
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values, extension_update_lock("andromeda").build(PostgresQueryBuilder).1);
        assert_ne!(values, extension_update_lock("andromeda_tenant_acme_0badc0de").build(PostgresQueryBuilder).1);
    }

    #[test]
    fn an_extension_installed_below_the_shipped_version_is_pending_down_to_a_patch() {
        let installed = ExtensionVersion::new(2, 30, 1);
        let shipped = ExtensionVersion::new(2, 30, 2);
        assert_eq!(
            pending_update(VersionedExtension::TimescaleDb, ExtensionPresence::Installed(installed), Some(shipped)),
            Some(PendingExtensionUpdate {
                extension: VersionedExtension::TimescaleDb,
                installed,
                shipped,
            })
        );
    }

    #[test]
    fn an_extension_at_or_above_the_shipped_version_absent_or_not_shipped_is_not_pending() {
        let shipped = ExtensionVersion::new(3, 6, 4);
        let settled = [
            (ExtensionPresence::Installed(shipped), Some(shipped)),
            (ExtensionPresence::Installed(ExtensionVersion::new(3, 6, 5)), Some(shipped)),
            (ExtensionPresence::Installed(ExtensionVersion::new(3, 5, 0)), None),
            (ExtensionPresence::Absent, Some(shipped)),
            (ExtensionPresence::Absent, None),
        ];
        for (installed, shipped) in settled {
            assert_eq!(
                pending_update(VersionedExtension::PostGis, installed, shipped),
                None,
                "{installed} against {shipped:?}"
            );
        }
    }

    #[test]
    fn a_driver_error_without_a_sqlstate_is_not_a_privilege_refusal() {
        assert!(!is_insufficient_privilege(&sqlx::Error::RowNotFound));
        assert!(!is_insufficient_privilege(&sqlx::Error::PoolTimedOut));
    }
}
