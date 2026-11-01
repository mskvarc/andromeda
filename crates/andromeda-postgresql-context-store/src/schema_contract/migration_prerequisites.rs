//! What a database must offer before any migrator runs against it.
//!
//! sqlx applies a migration set statement by statement, so a server that
//! cannot hold the schema would otherwise fail part-way through a migration,
//! on whichever statement first needs what it lacks, and leave the cause to be
//! read from a driver error. The server version, its encoding and the
//! extensions the migrations create are checked first instead, and a shortfall
//! is refused naming what is missing.

use crate::schema_contract::{
    error::SchemaContractError,
    server_requirements::{check_server, server_settings_statement},
    versioned_extension::VersionedExtension,
};
use andromeda_postgresql_seaquery::{introspection::extension_shipped_version, statement::Statement};
use sea_query::{Alias, SelectStatement};
use sqlx::{Executor, Postgres, Row};
use strum::IntoEnumIterator;

/// Refuse a database the migrations cannot run against: a server below the
/// version floor, a server encoding other than UTF-8, or an installation that
/// does not ship `PostGIS` or `TimescaleDB`.
///
/// Only availability is checked for the two extensions: the migrations create
/// an absent one at the version the installation ships, and the schema
/// contract checks that version against its floor once the migrations ran.
///
/// # Errors
///
/// Returns [`SchemaContractError::Database`] when the introspection query
/// fails or a column it returns does not decode,
/// [`SchemaContractError::PostgresBelowMinimum`] when the server is below the
/// version floor, [`SchemaContractError::ServerEncodingNotUtf8`] or
/// [`SchemaContractError::ServerEncodingUnreadable`] when its encoding is not
/// UTF-8, and [`SchemaContractError::ExtensionUnavailable`] when the
/// installation does not ship one of the two extensions.
pub async fn verify_migration_prerequisites<'e, E>(executor: E) -> Result<(), SchemaContractError>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = migration_prerequisites_statement().to_query().fetch_one(executor).await?;
    check_server(&row)?;
    VersionedExtension::iter().try_for_each(|extension| check_available(extension, row.try_get::<Option<&str>, _>(extension.shipped_column())?))
}

/// Read the server's version and encoding beside the version of each versioned
/// extension the installation ships, `NULL` for one it does not ship.
fn migration_prerequisites_statement() -> SelectStatement {
    VersionedExtension::iter().fold(server_settings_statement(), |mut select, extension| {
        select.expr_as(extension_shipped_version(extension.catalog_name()), Alias::new(extension.shipped_column()));
        select
    })
}

/// Refuse an extension the installation does not ship, which no migration can
/// create.
const fn check_available(extension: VersionedExtension, shipped: Option<&str>) -> Result<(), SchemaContractError> {
    match shipped {
        Some(_) => Ok(()),
        None => Err(SchemaContractError::ExtensionUnavailable { extension }),
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::{
        error::SchemaContractError,
        migration_prerequisites::{check_available, migration_prerequisites_statement},
        versioned_extension::VersionedExtension,
    };
    use sea_query::PostgresQueryBuilder;
    use strum::IntoEnumIterator;

    #[test]
    fn the_prerequisites_read_the_server_settings_and_what_the_installation_ships() {
        let sql = migration_prerequisites_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"current_setting('server_version_num')::integer AS "server_version_num""#),
            "{sql}"
        );
        assert!(sql.contains(r#"current_setting('server_encoding') AS "server_encoding""#), "{sql}");
        assert!(
            sql.contains(r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'postgis') AS "postgis_shipped_version""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "default_version" FROM "pg_available_extensions" WHERE "name" = 'timescaledb') AS "timescaledb_shipped_version""#),
            "{sql}"
        );
        assert!(!sql.contains(r#""pg_extension""#), "availability is not installation: {sql}");
    }

    #[test]
    fn an_extension_the_installation_does_not_ship_is_refused_naming_it() {
        for extension in VersionedExtension::iter() {
            assert!(
                matches!(
                    check_available(extension, None),
                    Err(SchemaContractError::ExtensionUnavailable { extension: refused }) if refused == extension
                ),
                "{extension}"
            );
        }
    }

    #[test]
    fn an_extension_the_installation_ships_at_any_version_is_available() {
        for extension in VersionedExtension::iter() {
            assert!(check_available(extension, Some("2.31.0")).is_ok(), "{extension}");
        }
    }

    #[test]
    fn an_unavailable_extension_is_reported_by_its_catalogue_name() {
        let refusal = SchemaContractError::ExtensionUnavailable {
            extension: VersionedExtension::TimescaleDb,
        };
        assert!(refusal.to_string().contains("timescaledb"), "{refusal}");
    }
}
