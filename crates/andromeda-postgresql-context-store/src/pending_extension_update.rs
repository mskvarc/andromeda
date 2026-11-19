//! Which versioned extensions of a database stand below the version the
//! server ships, and whether the session's role may update each one.
//!
//! `pg_extension` holds what the database installed and
//! `pg_available_extensions` what the server ships. `ALTER EXTENSION ...
//! UPDATE` takes the privileges of the extension's owner, so an update a role
//! lacking them would run is read as pending but not as one it may make.

use crate::{
    error::ContextStoreError,
    extension_standing::{ExtensionStanding, extension_standings},
    schema_contract::{
        error::SchemaContractError,
        server_requirements::{ExtensionPresence, ExtensionVersion},
        versioned_extension::VersionedExtension,
    },
    tracked_extension::TrackedExtension,
};
use andromeda_postgresql_seaquery::{introspection::current_role_owns_extension, statement::Statement};
use sea_query::{Alias, Query, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgRow};
use strum::IntoEnumIterator;

/// Whether the session's role may run `ALTER EXTENSION ... UPDATE` on an
/// installed extension, which takes the privileges of the extension's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePrivilege {
    /// The role owns the extension, inherits from its owner, or is a superuser.
    Held,
    /// Only the extension's owner may update it; the broker leaves it at the
    /// installed version for the schema contract's floor to judge.
    Lacking,
}

/// One versioned extension installed below the version the server ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingExtensionUpdate {
    /// The extension to update.
    pub extension: VersionedExtension,
    /// The version `pg_extension` records for this database.
    pub installed: ExtensionVersion,
    /// The version the server's installation ships, which the update reaches.
    pub shipped: ExtensionVersion,
    /// Whether the session's role may make the update.
    pub privilege: UpdatePrivilege,
}

/// A versioned extension installed below the version the server ships, before
/// the session's privilege over it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LaggingExtension {
    /// The extension.
    extension: VersionedExtension,
    /// The version `pg_extension` records for this database.
    installed: ExtensionVersion,
    /// The version the server's installation ships.
    shipped: ExtensionVersion,
}

impl LaggingExtension {
    /// The pending update, with the privilege the row of
    /// [`update_privileges_statement`] reads for the extension.
    fn pending(self, privileges: &PgRow) -> Result<PendingExtensionUpdate, ContextStoreError> {
        let owned: Option<bool> = privileges.try_get(self.extension.owned_column())?;
        let privilege = if owned == Some(true) {
            UpdatePrivilege::Held
        } else {
            UpdatePrivilege::Lacking
        };
        Ok(PendingExtensionUpdate {
            extension: self.extension,
            installed: self.installed,
            shipped: self.shipped,
            privilege,
        })
    }
}

/// The versioned extensions this database holds below the version the server
/// ships, each with whether the session's role may update it.
///
/// An extension the database does not hold is not pending: the migrations
/// create it at the shipped version. One the installation no longer ships is
/// not pending either, since nothing could update it. The privilege is read
/// only when something lags.
///
/// One installed above the version the server ships is refused: a server
/// running an older image than the one that last updated the database lacks
/// the installed version's library, so every session that loads the
/// extension fails, and no update reaches back.
///
/// Run it inside [`begin_without_timescaledb`], so the read leaves its
/// connection usable after an update and runs where the installed library is
/// missing.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails and
/// [`ContextStoreError::SchemaContract`] when a reported version cannot be
/// read or a versioned extension is installed above the shipped version.
pub async fn pending_extension_updates(connection: &mut PgConnection) -> Result<Vec<PendingExtensionUpdate>, ContextStoreError> {
    let lagging = extension_standings(&mut *connection)
        .await?
        .into_iter()
        .filter_map(|standing| lagging_extension(standing).transpose())
        .collect::<Result<Vec<LaggingExtension>, SchemaContractError>>()?;
    if lagging.is_empty() {
        return Ok(Vec::new());
    }
    let privileges = update_privileges_statement().to_query().fetch_one(connection).await?;
    lagging.into_iter().map(|lag| lag.pending(&privileges)).collect()
}

/// Whether any of `pending` is an update the session's role may make, which
/// a migration run would apply.
#[must_use]
pub fn any_updatable(pending: &[PendingExtensionUpdate]) -> bool {
    pending.iter().any(|update| update.privilege == UpdatePrivilege::Held)
}

/// The update one extension lags by, or `None` when it is absent, not shipped,
/// installed at the shipped version, or brought up to date by recreation
/// rather than by update.
///
/// A versioned extension installed above the shipped version is refused with
/// [`SchemaContractError::ExtensionAheadOfServer`].
fn lagging_extension(ExtensionStanding { extension, installed, shipped }: ExtensionStanding) -> Result<Option<LaggingExtension>, SchemaContractError> {
    match (extension, installed, shipped) {
        (TrackedExtension::Updated(extension), ExtensionPresence::Installed(installed), Some(shipped)) if installed < shipped => {
            Ok(Some(LaggingExtension { extension, installed, shipped }))
        }
        (TrackedExtension::Updated(extension), ExtensionPresence::Installed(installed), Some(shipped)) if installed > shipped => {
            Err(SchemaContractError::ExtensionAheadOfServer { extension, installed, shipped })
        }
        (TrackedExtension::Updated(_) | TrackedExtension::Recreated(_), ExtensionPresence::Installed(_) | ExtensionPresence::Absent, Some(_) | None) => {
            Ok(None)
        }
    }
}

/// Read, for every versioned extension, whether the session's role holds the
/// privileges of its owner, `NULL` for one the database does not hold.
fn update_privileges_statement() -> SelectStatement {
    VersionedExtension::iter().fold(Query::select(), |mut select, extension| {
        select.expr_as(current_role_owns_extension(extension.catalog_name()), Alias::new(extension.owned_column()));
        select
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        extension_standing::ExtensionStanding,
        pending_extension_update::{LaggingExtension, PendingExtensionUpdate, UpdatePrivilege, any_updatable, lagging_extension, update_privileges_statement},
        recreated_extension::RecreatedExtension,
        schema_contract::{
            error::SchemaContractError,
            server_requirements::{ExtensionPresence, ExtensionVersion},
            versioned_extension::VersionedExtension,
        },
        tracked_extension::TrackedExtension,
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn an_extension_installed_below_the_shipped_version_is_pending_down_to_a_patch() {
        let installed = ExtensionVersion::new(2, 30, 1);
        let shipped = ExtensionVersion::new(2, 30, 2);
        assert_eq!(
            lagging_extension(ExtensionStanding {
                extension: TrackedExtension::Updated(VersionedExtension::TimescaleDb),
                installed: ExtensionPresence::Installed(installed),
                shipped: Some(shipped),
            })
            .expect("a lagging extension is no refusal"),
            Some(LaggingExtension {
                extension: VersionedExtension::TimescaleDb,
                installed,
                shipped,
            })
        );
    }

    #[test]
    fn the_privilege_read_asks_whether_the_session_holds_each_versioned_extensions_owner() {
        let sql = update_privileges_statement().to_string(PostgresQueryBuilder);
        for clause in [
            r#"(SELECT pg_has_role(current_user, "extowner", 'USAGE') FROM "pg_extension" WHERE "extname" = 'postgis') AS "postgis_owned""#,
            r#"(SELECT pg_has_role(current_user, "extowner", 'USAGE') FROM "pg_extension" WHERE "extname" = 'timescaledb') AS "timescaledb_owned""#,
        ] {
            assert!(sql.contains(clause), "{clause} missing: {sql}");
        }
    }

    #[test]
    fn only_an_update_the_role_may_make_calls_for_a_migration_run() {
        let update = |privilege| PendingExtensionUpdate {
            extension: VersionedExtension::TimescaleDb,
            installed: ExtensionVersion::new(2, 31, 0),
            shipped: ExtensionVersion::new(2, 32, 0),
            privilege,
        };
        assert!(!any_updatable(&[]));
        assert!(!any_updatable(&[update(UpdatePrivilege::Lacking)]));
        assert!(any_updatable(&[update(UpdatePrivilege::Lacking), update(UpdatePrivilege::Held)]));
    }

    #[test]
    fn an_extension_at_the_shipped_version_absent_or_not_shipped_is_not_pending() {
        let shipped = ExtensionVersion::new(3, 6, 4);
        let settled = [
            (ExtensionPresence::Installed(shipped), Some(shipped)),
            (ExtensionPresence::Installed(ExtensionVersion::new(3, 5, 0)), None),
            (ExtensionPresence::Absent, Some(shipped)),
            (ExtensionPresence::Absent, None),
        ];
        for (installed, shipped) in settled {
            assert_eq!(
                lagging_extension(ExtensionStanding {
                    extension: TrackedExtension::Updated(VersionedExtension::PostGis),
                    installed,
                    shipped,
                })
                .expect("a settled extension is no refusal"),
                None,
                "{installed} against {shipped:?}"
            );
        }
    }

    #[test]
    fn a_versioned_extension_installed_above_the_shipped_version_is_refused_naming_both_versions() {
        let installed = ExtensionVersion::new(2, 32, 0);
        let shipped = ExtensionVersion::new(2, 31, 0);
        match lagging_extension(ExtensionStanding {
            extension: TrackedExtension::Updated(VersionedExtension::TimescaleDb),
            installed: ExtensionPresence::Installed(installed),
            shipped: Some(shipped),
        }) {
            Err(SchemaContractError::ExtensionAheadOfServer {
                extension: VersionedExtension::TimescaleDb,
                installed: refused,
                shipped: offered,
            }) => assert_eq!((refused, offered), (installed, shipped)),
            other => panic!("expected a refusal naming both versions, got {other:?}"),
        }
    }

    #[test]
    fn a_recreated_extension_above_the_shipped_version_is_left_to_the_recreation() {
        assert_eq!(
            lagging_extension(ExtensionStanding {
                extension: TrackedExtension::Recreated(RecreatedExtension::PgRepack),
                installed: ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 4)),
                shipped: Some(ExtensionVersion::new(1, 5, 3)),
            })
            .expect("a recreated extension is no refusal"),
            None
        );
    }

    #[test]
    fn a_recreated_extension_below_the_shipped_version_is_no_pending_update() {
        assert_eq!(
            lagging_extension(ExtensionStanding {
                extension: TrackedExtension::Recreated(RecreatedExtension::PgRepack),
                installed: ExtensionPresence::Installed(ExtensionVersion::new(1, 5, 2)),
                shipped: Some(ExtensionVersion::new(1, 5, 3)),
            })
            .expect("a recreated extension is no refusal"),
            None
        );
    }
}
