//! The number the `tenants.status` column stores a tenant's status as.
//!
//! Written as exhaustive matches in both directions, so a status added to the
//! tenancy vocabulary fails to compile here, where its persisted number is
//! decided.

use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_tenancy::status::TenantStatus;

/// Where a registered tenant stands, as its stored status names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredStanding {
    /// Registered for a creating operation; the tenant does not exist yet.
    Provisional,
    /// The tenant exists, with this administrative status.
    Established(TenantStatus),
}

/// The registry member the registry persists for an established tenant's
/// administrative status.
#[must_use]
pub const fn stored_status(status: TenantStatus) -> TenantStatusCode {
    match status {
        TenantStatus::Active => TenantStatusCode::Active,
        TenantStatus::Suspended => TenantStatusCode::Suspended,
    }
}

/// Where a tenant stands, as a stored registry member names it.
#[must_use]
pub const fn standing_of(code: TenantStatusCode) -> StoredStanding {
    match code {
        TenantStatusCode::Active => StoredStanding::Established(TenantStatus::Active),
        TenantStatusCode::Suspended => StoredStanding::Established(TenantStatus::Suspended),
        TenantStatusCode::Provisional => StoredStanding::Provisional,
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_registry::status_code::{StoredStanding, standing_of, stored_status};
    use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
    use andromeda_tenancy::status::TenantStatus;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_tenant_status_survives_a_round_trip_through_its_stored_number() {
        for status in TenantStatus::iter() {
            assert_eq!(standing_of(stored_status(status)), StoredStanding::Established(status));
        }
    }

    #[test]
    fn every_stored_number_names_a_standing_whose_label_it_carries() {
        for code in TenantStatusCode::ALL {
            let label = match standing_of(*code) {
                StoredStanding::Established(status) => status.to_string(),
                StoredStanding::Provisional => "provisional".to_string(),
            };
            assert_eq!(code.label(), label);
        }
    }
}
