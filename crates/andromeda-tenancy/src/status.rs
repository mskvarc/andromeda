//! The persisted administrative state of a registered tenant.

/// Administrative state of a tenant, as stored in the `tenants.status` column.
///
/// This is the write axis of tenant lifecycle control: an operator suspends a
/// tenant to take it out of service and resumes it to bring it back. It is kept
/// deliberately distinct from the computed wire [`TenantState`](crate::tenant_state::TenantState),
/// which folds this administrative state together with runtime residency into the
/// single read-only `status` a tenant resource exposes.
///
/// Carries no persistence derive: how a store spells `active` is the store's
/// business, and a `sqlx::Type` here is what used to make this enum unmovable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum TenantStatus {
    /// The tenant is in service and accepts requests.
    Active,
    /// The tenant is administratively suspended: data requests naming it are
    /// answered as if it did not exist (clause 5.5.10), and no runtime is
    /// admitted or woken for it until it is resumed.
    Suspended,
}

impl TenantStatus {
    /// Whether the tenant is administratively in service.
    ///
    /// This is the Boolean the tenant resource's client-writable `isActive`
    /// member (clause 5.2.12 Table 5.2.12-1) derives from: `Active` maps to
    /// `true`, `Suspended` to `false`.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, TenantStatus::Active)
    }
}

#[cfg(test)]
mod tests {
    use crate::status::TenantStatus;
    use std::str::FromStr;

    #[test]
    fn each_status_round_trips_through_its_lowercase_spelling() {
        for status in [TenantStatus::Active, TenantStatus::Suspended] {
            let rendered = status.to_string();
            assert_eq!(rendered, rendered.to_lowercase());
            pretty_assertions::assert_eq!(TenantStatus::from_str(&rendered).expect("parses"), status);
        }
    }

    #[test]
    fn only_the_active_status_is_in_service() {
        assert!(TenantStatus::Active.is_active());
        assert!(!TenantStatus::Suspended.is_active());
    }
}
