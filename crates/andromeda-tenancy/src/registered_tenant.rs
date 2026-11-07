//! A registry row, as far as admission needs to read it: whether the tenant
//! exists yet, and where its data lives.

use crate::tenant_record::TenantRecord;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;

/// A named tenant the registry holds a row for.
///
/// Clause 5.5.10 creates a tenant implicitly when an operation creating
/// information targets it. The broker registers the tenant and prepares its
/// database before that operation runs, so a row exists before the operation
/// is known to create anything: such a tenant is provisional, and is answered
/// as nonexistent until its database holds information.
#[derive(Debug, Clone)]
pub enum RegisteredTenant {
    /// Registered for a creating operation that has not yet been seen to create
    /// any information. Never exposed as a tenant resource.
    Provisional {
        /// The database prepared for the tenant, which may still be missing
        /// when provisioning was interrupted.
        database: TenantDatabaseName,
    },
    /// A tenant that exists, with its full registry row.
    Established(TenantRecord),
}

#[cfg(test)]
mod tests {
    use crate::{registered_tenant::RegisteredTenant, status::TenantStatus, tenant_record::TenantRecord};
    use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
    use chrono::{DateTime, Utc};

    #[test]
    fn a_provisional_tenant_carries_only_its_database() {
        let registered = RegisteredTenant::Provisional {
            database: TenantDatabaseName::from("andromeda_tenant_acme_0badc0de".to_string()),
        };
        assert!(matches!(registered, RegisteredTenant::Provisional { database } if database.as_str() == "andromeda_tenant_acme_0badc0de"));
    }

    #[test]
    fn an_established_tenant_carries_its_registry_row() {
        let name = TenantName::try_from("acme".to_string()).expect("a valid tenant name");
        let record = TenantRecord::new(
            name,
            TenantStatus::Suspended,
            TenantDatabaseName::from("andromeda_tenant_acme_0badc0de".to_string()),
            DateTime::<Utc>::UNIX_EPOCH,
            DateTime::<Utc>::UNIX_EPOCH,
        );
        let registered = RegisteredTenant::Established(record);
        assert!(matches!(registered, RegisteredTenant::Established(record) if record.status == TenantStatus::Suspended));
    }
}
