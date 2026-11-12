//! The PostgreSQL server the rehearsals create their databases on.

use crate::upgrade_database_error::UpgradeServerError;
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use std::env::{self, VarError};
use url::Url;

/// The environment variable naming the server: a PostgreSQL URL with an
/// administrative login, naming any database the login may connect to.
pub const SERVER_VARIABLE: &str = "ANDROMEDA_UPGRADE_TESTS__SERVER";

/// A PostgreSQL server a rehearsal may create databases on.
#[derive(Debug, Clone)]
pub struct UpgradeServer {
    /// The URL rehearsal databases are derived from.
    url: DatabaseUrl,
}

impl UpgradeServer {
    /// The server [`SERVER_VARIABLE`] names.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeServerError`] when the variable is unset, is not
    /// Unicode, or is not a URL.
    pub fn from_environment() -> Result<UpgradeServer, UpgradeServerError> {
        let text = env::var(SERVER_VARIABLE).map_err(|error| match error {
            VarError::NotPresent => UpgradeServerError::Unset,
            VarError::NotUnicode(_) => UpgradeServerError::NotUnicode,
        })?;
        UpgradeServer::from_text(&text)
    }

    /// The server a URL given as text names.
    ///
    /// # Errors
    ///
    /// Returns [`UpgradeServerError::Malformed`] when `text` is not a URL.
    pub fn from_text(text: &str) -> Result<UpgradeServer, UpgradeServerError> {
        let url = text.parse::<Url>().map_err(UpgradeServerError::Malformed)?;
        Ok(UpgradeServer { url: DatabaseUrl::new(url) })
    }

    /// The server's URL, from which a database's URL is derived.
    #[must_use]
    pub const fn url(&self) -> &DatabaseUrl {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use crate::{upgrade_database_error::UpgradeServerError, upgrade_server::UpgradeServer};
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_server_url_yields_the_url_of_a_database_on_the_same_server() -> Result<(), UpgradeServerError> {
        let server = UpgradeServer::from_text("postgres://admin:secret@postgres:5432/postgres")?;
        let derived = server.url().with_database_name(&TenantDatabaseName::from("upgrade_one".to_owned()));
        assert_eq!(derived.as_str(), "postgres://admin:secret@postgres:5432/upgrade_one");
        Ok(())
    }

    #[test]
    fn text_that_is_not_a_url_is_malformed() {
        assert!(matches!(UpgradeServer::from_text("not a url"), Err(UpgradeServerError::Malformed(_))));
    }
}
