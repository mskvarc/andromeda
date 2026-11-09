//! The relations the distributed subscription side keeps (clause 5.8.1.4).

use sea_query::Iden;

/// The tenant-scoped `subscription_companions` table: a local subscription and
/// the companion registration subscription mirroring it.
#[derive(Iden, Clone, Copy)]
pub enum SubscriptionCompanions {
    /// Table name: `subscription_companions`
    Table,
    /// The `subscriptions(subscription_key)` of the local entity subscription.
    LocalSubscriptionKey,
    /// The `subscriptions(subscription_key)` of the minted companion
    /// registration subscription.
    CompanionSubscriptionKey,
}

/// The tenant-scoped `forwarded_subscriptions` table: the reduced copy of a
/// local subscription placed at one registration's context source.
#[derive(Iden, Clone, Copy)]
pub enum ForwardedSubscriptions {
    /// Table name: `forwarded_subscriptions`
    Table,
    /// The `subscriptions(subscription_key)` of the local entity subscription.
    LocalSubscriptionKey,
    /// The registration whose source holds the copy.
    RegistrationId,
    /// The identifier of the copy at the source.
    RemoteSubscriptionId,
}

/// The tenant-scoped `forwarded_subscription_claims` table, holding the
/// placement of a copy whose source has not answered yet, with the instant
/// its lease ends. A copy without a row is settled.
#[derive(Iden, Clone, Copy)]
pub enum ForwardedSubscriptionClaims {
    /// Table name `forwarded_subscription_claims`
    Table,
    /// The `subscriptions(subscription_key)` of the local entity subscription.
    LocalSubscriptionKey,
    /// The instant the lease of the placement holding the claim ends.
    LeaseUntil,
    /// The registration whose source the copy is placed at.
    RegistrationId,
}

/// The platform `forwarded_subscription_routes` table: every minted
/// subscription identifier and the tenant it belongs to.
#[derive(Iden, Clone, Copy)]
pub enum ForwardedSubscriptionRoutes {
    /// Table name: `forwarded_subscription_routes`
    Table,
    /// The minted subscription identifier.
    MintedSubscriptionId,
    /// The owning tenant's `tenants(tenant_key)` surrogate, the unnamed default included.
    TenantKey,
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::{ForwardedSubscriptionClaims, ForwardedSubscriptionRoutes, ForwardedSubscriptions, SubscriptionCompanions};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn relation_and_column_names() {
        assert_eq!(SubscriptionCompanions::Table.to_string(), "subscription_companions");
        assert_eq!(SubscriptionCompanions::LocalSubscriptionKey.to_string(), "local_subscription_key");
        assert_eq!(SubscriptionCompanions::CompanionSubscriptionKey.to_string(), "companion_subscription_key");
        assert_eq!(ForwardedSubscriptions::Table.to_string(), "forwarded_subscriptions");
        assert_eq!(ForwardedSubscriptions::LocalSubscriptionKey.to_string(), "local_subscription_key");
        assert_eq!(ForwardedSubscriptions::RemoteSubscriptionId.to_string(), "remote_subscription_id");
        assert_eq!(ForwardedSubscriptionClaims::Table.to_string(), "forwarded_subscription_claims");
        assert_eq!(ForwardedSubscriptionClaims::LocalSubscriptionKey.to_string(), "local_subscription_key");
        assert_eq!(ForwardedSubscriptionClaims::LeaseUntil.to_string(), "lease_until");
        assert_eq!(ForwardedSubscriptionClaims::RegistrationId.to_string(), "registration_id");
        assert_eq!(ForwardedSubscriptionRoutes::Table.to_string(), "forwarded_subscription_routes");
        assert_eq!(ForwardedSubscriptionRoutes::MintedSubscriptionId.to_string(), "minted_subscription_id");
        assert_eq!(ForwardedSubscriptionRoutes::TenantKey.to_string(), "tenant_key");
    }
}
