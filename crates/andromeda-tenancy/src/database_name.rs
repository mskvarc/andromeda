//! Where one tenant's data lives.

use derive_more::{AsRef, Display, From, Into};

/// The store-side name of the database backing one tenant.
///
/// Opaque here on purpose: this crate decides *which* tenant needs a runtime
/// and never what the name means. Whether it is a PostgreSQL database, a
/// keyspace or a prefix is [`crate::database::TenantDatabase`]'s to know, and
/// giving it a type rather than passing a `String` is what stops a tenant name
/// and a database name being interchangeable at a call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Display, From, Into)]
#[as_ref(str)]
pub struct TenantDatabaseName(String);

impl TenantDatabaseName {
    /// The name as a string slice, for a store that has to spell it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::database_name::TenantDatabaseName;

    #[test]
    fn a_database_name_renders_as_the_store_spelled_it() {
        let name = TenantDatabaseName::from("andromeda_tenant_acme_0badc0de".to_string());
        pretty_assertions::assert_eq!(name.as_str(), "andromeda_tenant_acme_0badc0de");
        pretty_assertions::assert_eq!(name.to_string(), "andromeda_tenant_acme_0badc0de");
    }
}
