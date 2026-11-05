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
//! An optional extension that ships no update scripts, which no update reaches
//! a newer version of, is dropped and created again instead, under the same
//! lock and each statement on a connection of its own.
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
    extension_command::{ExtensionCommandRefusal, refusal, role_may_create, run_on_own_connection},
    extension_recreation::{apply_recreation, pending_extension_recreations},
    extension_standing::{ExtensionStanding, extension_standings},
    optional_extension::OptionalExtension,
    schema_contract::{
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
    tracked_extension::TrackedExtension,
};
use andromeda_postgresql_pool::schema_change_lock::schema_change_connect_options;
use andromeda_postgresql_seaquery::{
    advisory_lock::advisory_xact_lock,
    command::Command,
    extension_creation::ExtensionCreation,
    extension_update::ExtensionUpdate,
    introspection::{extension_shipped_version, extension_version},
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
};
use sea_query::{Alias, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};
use sqlx::{Acquire, Connection, PgConnection, Postgres, Row, Transaction, postgres::PgConnectOptions};
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
    Ok(extension_standings(connection).await?.into_iter().filter_map(pending_update).collect())
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
/// the version the server ships, drop and create again every recreated
/// extension [`pending_extension_recreations`] finds due, and install every
/// optional extension [`installable_optional_extensions`] finds due.
///
/// Run before anything else touches the database on this start, so no
/// connection the broker keeps has loaded the old `TimescaleDB` library: the
/// lock and the version read run without loading it, and each update runs on a
/// connection opened for it alone. An update the connecting role may not make
/// is logged and skipped, leaving the schema contract's floor to decide whether
/// the installed version still serves; a recreation or an installation refused
/// the same way is logged and skipped, leaving the feature that uses the
/// extension to report its absence, and a recreation whose drop finds
/// dependent objects is logged and left for a later open.
///
/// # Errors
///
/// Returns [`ContextStoreError::DatabaseUrlUnnamed`] when the URL names no
/// database, [`ContextStoreError::Database`] when a connection cannot be
/// opened, the lock or the read fails, or an update fails for a reason other
/// than privilege or a recreation or an installation fails the same way, and
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
/// recreated or installed, and apply all three, in one transaction that keeps
/// `TimescaleDB` unloaded on the lock holder.
async fn update_under_lock(lock_holder: &mut PgConnection, options: &PgConnectOptions) -> Result<(), ContextStoreError> {
    let database = options.get_database().ok_or(ContextStoreError::DatabaseUrlUnnamed)?;
    let mut transaction = begin_without_timescaledb(&mut *lock_holder).await?;
    extension_update_lock(database).to_query().execute(&mut *transaction).await?;
    for pending in pending_extension_updates(&mut transaction).await? {
        apply_update(options, pending).await?;
    }
    for pending in pending_extension_recreations(&mut transaction).await? {
        apply_recreation(options, pending).await?;
    }
    for extension in installable_optional_extensions(&mut transaction).await? {
        apply_installation(options, extension).await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Install one optional extension on a connection opened for it.
async fn apply_installation(options: &PgConnectOptions, extension: OptionalExtension) -> Result<(), ContextStoreError> {
    match run_on_own_connection(options, &ExtensionCreation::new(extension)).await {
        Ok(()) => {
            tracing::info!(extension = %extension, event = "extension_installed", "optional extension installed");
            Ok(())
        }
        Err(error) if refusal(&error) == Some(ExtensionCommandRefusal::InsufficientPrivilege) => {
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
        Err(error) if refusal(&error) == Some(ExtensionCommandRefusal::InsufficientPrivilege) => {
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

/// The update one extension needs, or `None` when it is absent, not shipped,
/// installed at or above the shipped version, or brought up to date by
/// recreation rather than by update.
///
/// An installed version above the shipped one is a server running an older
/// image than the one that last updated the database; no update reaches back,
/// and the schema contract's floor decides whether it still serves.
fn pending_update(ExtensionStanding { extension, installed, shipped }: ExtensionStanding) -> Option<PendingExtensionUpdate> {
    match (extension, installed, shipped) {
        (TrackedExtension::Updated(extension), ExtensionPresence::Installed(installed), Some(shipped)) if installed < shipped => {
            Some(PendingExtensionUpdate { extension, installed, shipped })
        }
        (TrackedExtension::Updated(_) | TrackedExtension::Recreated(_), ExtensionPresence::Installed(_) | ExtensionPresence::Absent, Some(_) | None) => None,
    }
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
    extension_shipped_version(name)
        .is_not_null()
        .and(extension_version(name).is_null())
        .and(role_may_create(name))
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
        extension_standing::ExtensionStanding,
        extension_update::{PendingExtensionUpdate, extension_update_lock, installable_extensions_statement, pending_update},
        recreated_extension::RecreatedExtension,
        schema_contract::{
            server_requirements::{ExtensionPresence, ExtensionVersion},
            versioned_extension::VersionedExtension,
        },
        tracked_extension::TrackedExtension,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::PostgresQueryBuilder;

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
    fn pg_repack_is_due_when_shipped_absent_and_the_role_is_a_superuser_or_the_version_trusted() {
        let sql = installable_extensions_statement().to_string(PostgresQueryBuilder);
        for clause in [
            r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'pg_repack') IS NOT NULL"#,
            r#"(SELECT "extversion" FROM "pg_extension" WHERE "extname" = 'pg_repack') IS NULL"#,
            r#"AND ((SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user)) OR COALESCE((SELECT "trusted" FROM "pg_available_extension_versions" WHERE "name" = 'pg_repack'"#,
            r#"AS "pg_repack_installable""#,
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
            pending_update(ExtensionStanding {
                extension: TrackedExtension::Updated(VersionedExtension::TimescaleDb),
                installed: ExtensionPresence::Installed(installed),
                shipped: Some(shipped),
            }),
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
                pending_update(ExtensionStanding {
                    extension: TrackedExtension::Updated(VersionedExtension::PostGis),
                    installed,
                    shipped,
                }),
                None,
                "{installed} against {shipped:?}"
            );
        }
    }

    #[test]
    fn a_recreated_extension_below_the_shipped_version_is_no_pending_update() {
        assert_eq!(
            pending_update(ExtensionStanding {
                extension: TrackedExtension::Recreated(RecreatedExtension::PgRepack),
                installed: ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 2)),
                shipped: Some(ExtensionVersion::new(1, 5, 3)),
            }),
            None
        );
    }
}
