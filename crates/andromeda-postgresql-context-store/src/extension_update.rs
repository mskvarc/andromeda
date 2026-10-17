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
//! Brokers starting together serialize on a transaction advisory lock keyed on
//! the database and read the versions again under it, so one of them updates
//! and the others find nothing left to do.

use crate::{
    error::ContextStoreError,
    schema_contract::{
        error::SchemaContractError,
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
    schema_ddl::extension::{DEFER_TIMESCALEDB_LOAD, update_extension},
};
use andromeda_postgresql_seaquery::{
    advisory_lock::advisory_xact_lock,
    introspection::{extension_shipped_version, extension_version},
};
use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{
    Acquire,
    ConnectOptions,
    Connection,
    Executor,
    PgConnection,
    Postgres,
    Row,
    Transaction,
    error::DatabaseError,
    postgres::{PgConnectOptions, PgRow},
};
use strum::IntoEnumIterator;
use url::Url;

/// SQLSTATE `42501`, `insufficient_privilege`: the connecting role does not own
/// the extension, or may not run an untrusted extension's update script.
const INSUFFICIENT_PRIVILEGE: &str = "42501";

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
    transaction.execute(sqlx::AssertSqlSafe(DEFER_TIMESCALEDB_LOAD)).await?;
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
    let (sql, values) = extension_versions_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    VersionedExtension::iter()
        .filter_map(|extension| {
            read_versions(&row, extension)
                .map(|(installed, shipped)| pending_update(extension, installed, shipped))
                .transpose()
        })
        .collect()
}

/// Update every versioned extension of the database `database_url` names to
/// the version the server ships.
///
/// Run before anything else touches the database on this start, so no
/// connection the broker keeps has loaded the old `TimescaleDB` library: the
/// lock and the version read run without loading it, and each update runs on a
/// connection opened for it alone. An update the connecting role may not make
/// is logged and skipped, leaving the schema contract's floor to decide whether
/// the installed version still serves.
///
/// # Errors
///
/// Returns [`ContextStoreError::DatabaseUrlUnnamed`] when the URL names no
/// database, [`ContextStoreError::Database`] when a connection cannot be
/// opened, the lock or the read fails, or an update fails for a reason other
/// than privilege, and [`ContextStoreError::SchemaContract`] when a reported
/// version cannot be read.
pub async fn update_extensions(database_url: &Url) -> Result<(), ContextStoreError> {
    let options = PgConnectOptions::from_url(database_url)?;
    let mut lock_holder = PgConnection::connect_with(&options).await?;
    let outcome = update_under_lock(&mut lock_holder, &options).await;
    lock_holder.close().await?;
    outcome
}

/// Take the database's update lock, read what is pending, and apply it, all
/// in one transaction that keeps `TimescaleDB` unloaded on the lock holder.
async fn update_under_lock(lock_holder: &mut PgConnection, options: &PgConnectOptions) -> Result<(), ContextStoreError> {
    let database = options.get_database().ok_or(ContextStoreError::DatabaseUrlUnnamed)?;
    let mut transaction = begin_without_timescaledb(&mut *lock_holder).await?;
    let (lock_sql, lock_values) = extension_update_lock(database).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(lock_sql.as_str()), lock_values)
        .execute(&mut *transaction)
        .await?;
    for pending in pending_extension_updates(&mut transaction).await? {
        apply_update(options, pending).await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Run one update as the first statement of a connection opened for it.
async fn apply_update(options: &PgConnectOptions, pending: PendingExtensionUpdate) -> Result<(), ContextStoreError> {
    let mut connection = PgConnection::connect_with(options).await?;
    let outcome = connection.execute(sqlx::AssertSqlSafe(update_extension(pending.extension))).await;
    connection.close().await?;
    match outcome {
        Ok(_) => {
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

/// Whether a failed update was refused for privilege rather than failing.
fn is_insufficient_privilege(error: &sqlx::Error) -> bool {
    error.as_database_error().and_then(DatabaseError::code).as_deref() == Some(INSUFFICIENT_PRIVILEGE)
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
        extension_update::{PendingExtensionUpdate, extension_update_lock, extension_versions_statement, is_insufficient_privilege, pending_update},
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
