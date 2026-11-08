//! The base URL every tenant database connection is derived from.
//!
//! A tenant's database is reached at the configured base URL with its own
//! database name as the path, so the derivation is decided once for every
//! store that connects to a tenant database by name.

use crate::sql_state::SqlState;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use thiserror::Error;
use url::Url;

/// A database URL whose path names a database, the base each tenant database
/// URL is built from.
#[derive(Clone, Debug, derive_more::AsRef)]
pub struct DatabaseUrl(Url);

/// The URL carries no path segment, so it names no database.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the database URL names no database")]
pub struct DatabaseUrlUnnamed;

impl DatabaseUrl {
    /// Wrap a validated database URL.
    #[must_use]
    pub const fn new(url: Url) -> DatabaseUrl {
        DatabaseUrl(url)
    }

    /// The database the URL's path names.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseUrlUnnamed`] when the URL carries no path segment.
    pub fn database_name(&self) -> Result<TenantDatabaseName, DatabaseUrlUnnamed> {
        let path = self.0.path().trim_start_matches('/');
        if path.is_empty() {
            return Err(DatabaseUrlUnnamed);
        }
        Ok(TenantDatabaseName::from(path.to_owned()))
    }

    /// The URL of `database_name` on the same server, with the same
    /// credentials and parameters.
    #[must_use]
    pub fn with_database_name(&self, database_name: &TenantDatabaseName) -> Url {
        // clone: the base URL is reused for other tenants, so set_path mutates a copy.
        let mut url = self.0.clone();
        url.set_path(&format!("/{database_name}"));
        url
    }
}

/// Whether a connection failed because the database it named does not exist
/// (SQLSTATE 3D000), which a tenant deprovisioned under a stale reference
/// produces.
#[must_use]
pub fn names_missing_database(error: &sqlx::Error) -> bool {
    SqlState::of(error) == Some(SqlState::InvalidCatalogName)
}

#[cfg(test)]
mod tests {
    use crate::{
        database_url::{DatabaseUrl, DatabaseUrlUnnamed, names_missing_database},
        fake_database_error::FakeDatabaseError,
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tenant_url_replaces_only_the_database_name() {
        let parsed = DatabaseUrl::new(
            "postgres://postgres:postgres@localhost:5432/postgres?sslmode=disable"
                .parse()
                .expect("database URL"),
        );
        assert_eq!(parsed.database_name().map(|name| name.as_str().to_owned()), Ok("postgres".to_owned()));
        let rebuilt = parsed.with_database_name(&TenantDatabaseName::from("tenant_db".to_owned()));
        assert_eq!(rebuilt.as_str(), "postgres://postgres:postgres@localhost:5432/tenant_db?sslmode=disable");
    }

    #[test]
    fn a_url_without_a_path_names_no_database() {
        let parsed = DatabaseUrl::new("postgres://postgres:postgres@localhost:5432".parse().expect("database URL"));
        assert_eq!(parsed.database_name().map(|name| name.as_str().to_owned()), Err(DatabaseUrlUnnamed));
    }

    #[test]
    fn only_invalid_catalog_name_says_the_database_is_missing() {
        assert!(names_missing_database(&FakeDatabaseError::new("no database", "3D000").into_sqlx()));
        assert!(!names_missing_database(&FakeDatabaseError::new("no table", "42P01").into_sqlx()));
        assert!(!names_missing_database(&sqlx::Error::RowNotFound));
    }
}
