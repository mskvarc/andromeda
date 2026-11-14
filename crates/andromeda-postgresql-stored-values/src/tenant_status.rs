//! The persisted administrative status of one tenant.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The status of a tenant in the platform registry: whether it exists yet
    /// and, once it does, its administrative status.
    ///
    /// Tenant status is an Andromeda administrative concept, so the labels are
    /// kebab-case.
    TenantStatusCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 3,
        frozen_digest: 0x8a83_8e65_2f83_40cc,
        codes: {
            /// The tenant is in service.
            Active = 1, "active";
            /// The tenant is administratively suspended.
            Suspended = 2, "suspended";
            /// The tenant is registered for a creating operation and does not
            /// exist until its database holds information.
            Provisional = 3, "provisional";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_status::TenantStatusCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(TenantStatusCode::ALL.len(), 3);
        assert_eq!(TenantStatusCode::Active.code(), 1);
        assert_eq!(TenantStatusCode::Suspended.code(), 2);
        assert_eq!(TenantStatusCode::Provisional.code(), 3);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(TenantStatusCode::try_from(0).is_err());
        assert!(TenantStatusCode::try_from(4).is_err());
    }
}
