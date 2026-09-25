//! The number the `tenants.status` column stores a tenant's status as.
//!
//! Written as exhaustive matches in both directions, so a status added to the
//! tenancy vocabulary fails to compile here, where its persisted number is
//! decided.

use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_tenancy::status::TenantStatus;

/// The registry member the registry persists for a tenant status.
#[must_use]
pub const fn stored_status(status: TenantStatus) -> TenantStatusCode {
    match status {
        TenantStatus::Active => TenantStatusCode::Active,
        TenantStatus::Suspended => TenantStatusCode::Suspended,
    }
}

/// The tenant status a stored registry member names.
#[must_use]
pub const fn status_of(code: TenantStatusCode) -> TenantStatus {
    match code {
        TenantStatusCode::Active => TenantStatus::Active,
        TenantStatusCode::Suspended => TenantStatus::Suspended,
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_registry::status_code::{status_of, stored_status};
    use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
    use andromeda_tenancy::status::TenantStatus;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_tenant_status_survives_a_round_trip_through_its_stored_number() {
        for status in TenantStatus::iter() {
            assert_eq!(status_of(stored_status(status)), status);
        }
    }

    #[test]
    fn every_stored_number_names_a_status_whose_label_it_carries() {
        for code in TenantStatusCode::ALL {
            assert_eq!(code.label(), status_of(*code).to_string());
        }
    }
}
