//! The PostgreSQL database holding one tenant, as the broker names it under `options=usage`: what a test addresses
//! the tenant's pooler pool and server backends by.

use derive_more::{Display, From};
use serde::Deserialize;

/// The name of the database holding one tenant's data, as the broker's tenant resource reports it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Display, From)]
#[serde(transparent)]
pub struct DatabaseName(String);

impl DatabaseName {
    /// The name as the server and the pooler spell it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::database_name::DatabaseName;
    use googletest::prelude::*;

    #[gtest]
    fn a_database_name_reads_from_a_bare_string() -> Result<()> {
        let name: DatabaseName = serde_json::from_str(r#""andromeda_tenant_acme_0badc0de""#)?;
        expect_that!(name.as_str(), eq("andromeda_tenant_acme_0badc0de"));
        verify_that!(name.to_string(), eq("andromeda_tenant_acme_0badc0de"))
    }
}
