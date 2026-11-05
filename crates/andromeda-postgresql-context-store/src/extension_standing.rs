//! Reading each tracked extension's installed version in a database beside the
//! version the server's installation ships.
//!
//! `pg_extension` holds what one database installed and
//! `pg_available_extensions` what the server ships, which every database of
//! the server shares; one row reads both for every tracked extension.

use crate::{
    error::ContextStoreError,
    schema_contract::{
        error::SchemaContractError,
        server_requirements::{ExtensionPresence, ExtensionVersion},
    },
    tracked_extension::TrackedExtension,
};
use andromeda_postgresql_seaquery::{
    introspection::{extension_shipped_version, extension_version},
    statement::Statement,
};
use sea_query::{Alias, Query, SelectStatement};
use sqlx::{Executor, Postgres, Row, postgres::PgRow};

/// One tracked extension's installed version in a database, and the version
/// the server's installation ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionStanding {
    /// The extension.
    pub extension: TrackedExtension,
    /// What `pg_extension` records for this database.
    pub installed: ExtensionPresence,
    /// The version the server's installation ships, `None` when it ships none.
    pub shipped: Option<ExtensionVersion>,
}

/// Every tracked extension's installed version in the database `executor`
/// reaches, beside the version the server ships.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails and
/// [`ContextStoreError::SchemaContract`] when a reported version cannot be
/// read.
pub async fn extension_standings<'e, E>(executor: E) -> Result<Vec<ExtensionStanding>, ContextStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = extension_versions_statement().to_query().fetch_one(executor).await?;
    TrackedExtension::all().map(|extension| read_standing(&row, extension)).collect()
}

/// Add `extension`'s installed and shipped version to `select`, under the
/// columns [`read_standing`] reads.
pub(crate) fn select_standing(select: &mut SelectStatement, extension: TrackedExtension) {
    select
        .expr_as(extension_version(extension.catalog_name()), Alias::new(extension.installed_column()))
        .expr_as(extension_shipped_version(extension.catalog_name()), Alias::new(extension.shipped_column()));
}

/// One extension's standing from a row [`select_standing`] projected.
pub(crate) fn read_standing(row: &PgRow, extension: TrackedExtension) -> Result<ExtensionStanding, ContextStoreError> {
    let installed = ExtensionPresence::try_from(row.try_get::<Option<String>, _>(extension.installed_column())?).map_err(SchemaContractError::from)?;
    let shipped = row
        .try_get::<Option<String>, _>(extension.shipped_column())?
        .map(|raw| ExtensionVersion::try_from(raw.as_str()))
        .transpose()
        .map_err(SchemaContractError::from)?;
    Ok(ExtensionStanding { extension, installed, shipped })
}

/// Read every tracked extension's installed and shipped version in one row.
fn extension_versions_statement() -> SelectStatement {
    TrackedExtension::all().fold(Query::select(), |mut select, extension| {
        select_standing(&mut select, extension);
        select
    })
}

#[cfg(test)]
mod tests {
    use crate::extension_standing::extension_versions_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_version_read_takes_each_tracked_extensions_installed_and_shipped_version() {
        let sql = extension_versions_statement().to_string(PostgresQueryBuilder);
        for column in [
            r#"AS "postgis_version""#,
            r#"AS "postgis_shipped_version""#,
            r#"AS "timescaledb_version""#,
            r#"AS "timescaledb_shipped_version""#,
            r#"AS "pg_repack_version""#,
            r#"AS "pg_repack_shipped_version""#,
        ] {
            assert!(sql.contains(column), "{column} missing: {sql}");
        }
        assert!(sql.contains(r#"FROM "pg_extension" WHERE "extname" = 'timescaledb'"#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_available_extensions" WHERE "name" = 'postgis'"#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_extension" WHERE "extname" = 'pg_repack'"#), "{sql}");
    }
}
