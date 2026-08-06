//! Whether a registered tenant is serving requests.

/// Status of a tenant in the registry.
///
/// Carries no persistence derive: how a store spells `active` is the store's
/// business, and a `sqlx::Type` here is what used to make this enum unmovable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum TenantStatus {
    /// Tenant is active and accepting requests.
    Active,
    /// Tenant is suspended and should not serve requests.
    Suspended,
}

#[cfg(test)]
mod tests {
    use crate::status::TenantStatus;
    use std::str::FromStr;

    #[test]
    fn every_status_round_trips_through_its_lowercase_spelling() {
        for status in [TenantStatus::Active, TenantStatus::Suspended] {
            let rendered = status.to_string();
            assert_eq!(rendered, rendered.to_lowercase());
            pretty_assertions::assert_eq!(TenantStatus::from_str(&rendered).expect("parses"), status);
        }
    }
}
