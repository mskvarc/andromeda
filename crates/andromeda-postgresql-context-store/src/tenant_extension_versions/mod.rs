//! The `tenant_extension_versions` table: the versioned extensions each tenant
//! database held after the broker last opened it, and the tenants a wake pass
//! loads to bring up to date.
//!
//! A database's installed extension versions live only in its own catalogue,
//! so after an image upgrade the platform database cannot tell which tenants
//! are behind without this record. Every open writes the record of the tenant
//! it opened once its extensions are up to date, and the wake pass reads it to
//! load the tenants still behind, the furthest behind first.

mod catch_up;
mod catch_up_rank;

use crate::{
    error::ContextStoreError,
    extension_update::{ExtensionStanding, extension_standings},
    schema_contract::server_requirements::{ExtensionPresence, ExtensionVersion},
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_schema::tenancy::{TenantExtensionVersions, Tenants};
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{Expr, ExprTrait, InsertStatement, OnConflict, Query, QueryStatementBuilder, SimpleExpr, error::Error as SeaQueryError};
use sqlx::PgPool;

/// The tenant extension versions, backed by the platform database.
pub struct PostgresTenantExtensionVersions {
    /// The platform pool the table lives in.
    main_pool: PgPool,
}

impl PostgresTenantExtensionVersions {
    /// The record over the platform pool.
    #[must_use]
    pub const fn new(main_pool: PgPool) -> PostgresTenantExtensionVersions {
        PostgresTenantExtensionVersions { main_pool }
    }

    /// Record the versioned extensions the database behind `tenant_pool`
    /// holds, and the versions the server ships, as the open of `database`
    /// left them.
    ///
    /// Run after the open brought the extensions up to date, so the record
    /// says which server version the open's one update attempt ran against.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the tenant read or the
    /// platform write fails, [`ContextStoreError::SchemaContract`] when a
    /// reported version cannot be read, and
    /// [`ContextStoreError::StatementBuild`] when the write cannot be
    /// assembled.
    pub async fn record(&self, database: &TenantDatabaseName, tenant_pool: &PgPool) -> Result<(), ContextStoreError> {
        let standings = extension_standings(tenant_pool).await?;
        record_statement(database, &standings)?.to_query().execute(&self.main_pool).await?;
        Ok(())
    }
}

/// Upsert one row per extension for the tenant whose database is `database`,
/// resolving its surrogate key by database name.
fn record_statement(database: &TenantDatabaseName, standings: &[ExtensionStanding]) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement.into_table(TenantExtensionVersions::Table).columns([
        TenantExtensionVersions::TenantKey,
        TenantExtensionVersions::ReconciledAt,
        TenantExtensionVersions::Extension,
        TenantExtensionVersions::InstalledVersion,
        TenantExtensionVersions::ShippedVersion,
    ]);
    for standing in standings {
        statement.values([
            tenant_key_of(database),
            Expr::current_timestamp(),
            Expr::val(standing.extension.catalog_name()),
            Expr::val(installed_text(standing.installed)),
            Expr::val(standing.shipped.as_ref().map(ExtensionVersion::to_string)),
        ])?;
    }
    statement.on_conflict(
        OnConflict::columns([TenantExtensionVersions::TenantKey, TenantExtensionVersions::Extension])
            .update_columns([
                TenantExtensionVersions::ReconciledAt,
                TenantExtensionVersions::InstalledVersion,
                TenantExtensionVersions::ShippedVersion,
            ])
            .to_owned(),
    );
    Ok(statement)
}

/// The stored spelling of an installed version, `None` when not installed.
fn installed_text(installed: ExtensionPresence) -> Option<String> {
    match installed {
        ExtensionPresence::Installed(version) => Some(version.to_string()),
        ExtensionPresence::Absent => None,
    }
}

/// The surrogate key of the tenant whose database is `database`.
fn tenant_key_of(database: &TenantDatabaseName) -> SimpleExpr {
    let select = Query::select()
        .column(Tenants::TenantKey)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::DatabaseName).eq(database.as_str()))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(select.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::{
        extension_update::ExtensionStanding,
        schema_contract::{
            server_requirements::{ExtensionPresence, ExtensionVersion},
            versioned_extension::VersionedExtension,
        },
        tenant_extension_versions::record_statement,
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use sea_query::{PostgresQueryBuilder, error::Error as SeaQueryError};

    #[test]
    fn the_record_writes_one_row_per_extension_keyed_by_the_tenants_database() -> Result<(), SeaQueryError> {
        let standings = [
            ExtensionStanding {
                extension: VersionedExtension::PostGis,
                installed: ExtensionPresence::Installed(ExtensionVersion::new(3, 5, 2)),
                shipped: Some(ExtensionVersion::new(3, 6, 0)),
            },
            ExtensionStanding {
                extension: VersionedExtension::TimescaleDb,
                installed: ExtensionPresence::Absent,
                shipped: None,
            },
        ];
        let sql = record_statement(&TenantDatabaseName::from("andromeda_tenant_acme".to_owned()), &standings)?.to_string(PostgresQueryBuilder);
        for fragment in [
            r#"INSERT INTO "tenant_extension_versions" ("tenant_key", "reconciled_at", "extension", "installed_version", "shipped_version")"#,
            r#"(SELECT "tenant_key" FROM "tenants" WHERE "database_name" = 'andromeda_tenant_acme')"#,
            r"CURRENT_TIMESTAMP, 'postgis', '3.5.2', '3.6.0')",
            r"CURRENT_TIMESTAMP, 'timescaledb', NULL, NULL)",
            r#"ON CONFLICT ("tenant_key", "extension") DO UPDATE SET "reconciled_at" = "excluded"."reconciled_at", "installed_version" = "excluded"."installed_version", "shipped_version" = "excluded"."shipped_version""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        Ok(())
    }
}
