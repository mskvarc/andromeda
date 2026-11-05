//! Bringing a recreated extension to the version the server ships, by
//! dropping it and creating it again.
//!
//! The drop is `RESTRICT`. While a `pg_repack` run is in progress, or after
//! one that did not finish left its log tables and types behind, objects
//! outside the extension depend on its `repack` schema and the drop is refused:
//! the installed version stays, and a later open tries again. A cascading drop
//! would take the run's objects with it. Once the drop succeeds the extension
//! is absent until the create commits; a create that fails leaves it absent,
//! and the next open installs it as it installs any optional extension.
//!
//! Recreating is due only when the session's role may create the extension,
//! so a database whose role cannot does not read as needing reconciliation on
//! every open.

use crate::{
    error::ContextStoreError,
    extension_command::{ExtensionCommandRefusal, refusal, role_may_create, run_on_own_connection},
    extension_standing::{read_standing, select_standing},
    recreated_extension::RecreatedExtension,
    schema_contract::server_requirements::{ExtensionPresence, ExtensionVersion},
    tracked_extension::TrackedExtension,
};
use andromeda_postgresql_seaquery::{extension_creation::ExtensionCreation, extension_removal::ExtensionRemoval, statement::Statement};
use sea_query::{Alias, Query, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgConnectOptions};
use strum::IntoEnumIterator;

/// One recreated extension installed below the version the server ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingExtensionRecreation {
    /// The extension to drop and create again.
    pub extension: RecreatedExtension,
    /// The version `pg_extension` records for this database.
    pub installed: ExtensionVersion,
    /// The version the server's installation ships, which the create installs.
    pub shipped: ExtensionVersion,
}

/// Whether the session's role may create an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreationRight {
    /// The role is a superuser, or the shipped version is trusted.
    Granted,
    /// The role may not create the extension.
    Withheld,
}

/// How a `RESTRICT` drop ended.
#[derive(Debug)]
enum RemovalOutcome {
    /// The extension is dropped.
    Removed,
    /// Objects outside the extension depend on it; the drop is left for a
    /// later open.
    Deferred(sqlx::Error),
    /// The role may not drop the extension.
    Refused(sqlx::Error),
}

/// The recreated extensions this database holds below the version the server
/// ships and the session's role may create again.
///
/// An extension the database does not hold is not pending: the installation
/// of optional extensions creates it.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails or a column
/// does not decode, and [`ContextStoreError::SchemaContract`] when a reported
/// version cannot be read.
pub async fn pending_extension_recreations(connection: &mut PgConnection) -> Result<Vec<PendingExtensionRecreation>, ContextStoreError> {
    let row = recreation_statement().to_query().fetch_one(connection).await?;
    RecreatedExtension::iter()
        .map(|extension| -> Result<Option<PendingExtensionRecreation>, ContextStoreError> {
            let standing = read_standing(&row, TrackedExtension::Recreated(extension))?;
            let right = if row.try_get::<bool, _>(extension.creatable_column())? {
                CreationRight::Granted
            } else {
                CreationRight::Withheld
            };
            Ok(pending_recreation(extension, standing.installed, standing.shipped, right))
        })
        .filter_map(Result::transpose)
        .collect()
}

/// Drop one recreated extension and create it again, each as the first
/// statement of a connection opened for it.
///
/// A drop refused for dependent objects or for privilege is logged and leaves
/// the installed version; a create refused for privilege is logged and leaves
/// the extension absent.
pub(crate) async fn apply_recreation(options: &PgConnectOptions, pending: PendingExtensionRecreation) -> Result<(), ContextStoreError> {
    let extension = pending.extension.optional();
    match removal_outcome(run_on_own_connection(options, &ExtensionRemoval::new(extension)).await)? {
        RemovalOutcome::Removed => create_again(options, pending).await,
        RemovalOutcome::Deferred(error) => {
            tracing::warn!(
                extension = %extension,
                installed = %pending.installed,
                shipped = %pending.shipped,
                error = %error,
                event = "extension_recreate_deferred",
                "objects outside the extension depend on it, as a repack run's do; the installed version is kept and a later open tries again"
            );
            Ok(())
        }
        RemovalOutcome::Refused(error) => {
            tracing::warn!(
                extension = %extension,
                installed = %pending.installed,
                shipped = %pending.shipped,
                error = %error,
                event = "extension_recreate_refused",
                "extension recreation needs a role that may drop and create it; the installed version is kept"
            );
            Ok(())
        }
    }
}

/// Create a just-dropped extension again at the version the server ships.
async fn create_again(options: &PgConnectOptions, pending: PendingExtensionRecreation) -> Result<(), ContextStoreError> {
    let extension = pending.extension.optional();
    match run_on_own_connection(options, &ExtensionCreation::new(extension)).await {
        Ok(()) => {
            tracing::info!(
                extension = %extension,
                from = %pending.installed,
                to = %pending.shipped,
                event = "extension_recreated",
                "extension dropped and created again at the version the server ships"
            );
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

/// How a drop's result ends the recreation: dropped, left for a later open, or
/// refused, with any other failure returned.
fn removal_outcome(result: Result<(), sqlx::Error>) -> Result<RemovalOutcome, sqlx::Error> {
    match result {
        Ok(()) => Ok(RemovalOutcome::Removed),
        Err(error) => match refusal(&error) {
            Some(ExtensionCommandRefusal::DependentObjectsStillExist) => Ok(RemovalOutcome::Deferred(error)),
            Some(ExtensionCommandRefusal::InsufficientPrivilege) => Ok(RemovalOutcome::Refused(error)),
            None => Err(error),
        },
    }
}

/// The recreation one extension needs, or `None` when it is absent, not
/// shipped, installed at or above the shipped version, or not creatable by the
/// session's role.
///
/// An installed version above the shipped one is a server running an older
/// image than the one that last recreated it; recreating would move it back.
fn pending_recreation(
    extension: RecreatedExtension,
    installed: ExtensionPresence,
    shipped: Option<ExtensionVersion>,
    right: CreationRight,
) -> Option<PendingExtensionRecreation> {
    match (installed, shipped, right) {
        (ExtensionPresence::Installed(installed), Some(shipped), CreationRight::Granted) if installed < shipped => {
            Some(PendingExtensionRecreation { extension, installed, shipped })
        }
        (ExtensionPresence::Installed(_) | ExtensionPresence::Absent, Some(_) | None, CreationRight::Granted | CreationRight::Withheld) => None,
    }
}

/// Read, for every recreated extension, its installed and shipped version and
/// whether the session's role may create it.
fn recreation_statement() -> SelectStatement {
    RecreatedExtension::iter().fold(Query::select(), |mut select, extension| {
        select_standing(&mut select, TrackedExtension::Recreated(extension));
        select.expr_as(role_may_create(extension.catalog_name()), Alias::new(extension.creatable_column()));
        select
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        extension_recreation::{CreationRight, PendingExtensionRecreation, RemovalOutcome, pending_recreation, recreation_statement, removal_outcome},
        recreated_extension::RecreatedExtension,
        schema_contract::server_requirements::{ExtensionPresence, ExtensionVersion},
    };
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    const SHIPPED: ExtensionVersion = ExtensionVersion::new(1, 5, 3);

    #[test]
    fn pg_repack_installed_a_patch_below_the_shipped_version_is_recreated() {
        let installed = ExtensionVersion::new(1, 5, 2);
        assert_eq!(
            pending_recreation(
                RecreatedExtension::PgRepack,
                ExtensionPresence::Installed(installed),
                Some(SHIPPED),
                CreationRight::Granted
            ),
            Some(PendingExtensionRecreation {
                extension: RecreatedExtension::PgRepack,
                installed,
                shipped: SHIPPED,
            })
        );
    }

    #[test]
    fn pg_repack_at_or_above_the_shipped_version_absent_or_not_shipped_is_not_recreated() {
        let settled = [
            (ExtensionPresence::Installed(SHIPPED), Some(SHIPPED)),
            (ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 4)), Some(SHIPPED)),
            (ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 2)), None),
            (ExtensionPresence::Absent, Some(SHIPPED)),
            (ExtensionPresence::Absent, None),
        ];
        for (installed, shipped) in settled {
            assert_eq!(
                pending_recreation(RecreatedExtension::PgRepack, installed, shipped, CreationRight::Granted),
                None,
                "{installed} against {shipped:?}"
            );
        }
    }

    #[test]
    fn pg_repack_below_the_shipped_version_is_not_recreated_by_a_role_that_may_not_create_it() {
        assert_eq!(
            pending_recreation(
                RecreatedExtension::PgRepack,
                ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 2)),
                Some(SHIPPED),
                CreationRight::Withheld
            ),
            None
        );
    }

    #[test]
    fn a_drop_refused_for_dependent_objects_is_deferred_not_failed() {
        let outcome = removal_outcome(Err(FakeDatabaseError::new(
            "cannot drop extension pg_repack because other objects depend on it",
            "2BP01",
        )
        .into_sqlx()));
        assert!(matches!(outcome, Ok(RemovalOutcome::Deferred(_))), "{outcome:?}");
    }

    #[test]
    fn a_drop_refused_for_privilege_is_a_refusal_not_a_failure() {
        let outcome = removal_outcome(Err(FakeDatabaseError::new("must be owner of extension pg_repack", "42501").into_sqlx()));
        assert!(matches!(outcome, Ok(RemovalOutcome::Refused(_))), "{outcome:?}");
    }

    #[test]
    fn a_drop_that_succeeds_removes_and_any_other_failure_is_returned() {
        assert!(matches!(removal_outcome(Ok(())), Ok(RemovalOutcome::Removed)));
        let outcome = removal_outcome(Err(FakeDatabaseError::new("deadlock", "40P01").into_sqlx()));
        assert!(matches!(outcome, Err(sqlx::Error::Database(_))), "{outcome:?}");
    }

    #[test]
    fn the_recreation_read_takes_pg_repacks_versions_and_whether_the_role_may_create_it() {
        let sql = recreation_statement().to_string(PostgresQueryBuilder);
        for clause in [
            r#"(SELECT "extversion" FROM "pg_extension" WHERE "extname" = 'pg_repack') AS "pg_repack_version""#,
            r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'pg_repack') AS "pg_repack_shipped_version""#,
            r#"(SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user))"#,
            r#"AS "pg_repack_creatable""#,
        ] {
            assert!(sql.contains(clause), "{clause} missing: {sql}");
        }
    }
}
