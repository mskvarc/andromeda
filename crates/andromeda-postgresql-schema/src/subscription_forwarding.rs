//! The relations the distributed subscription side keeps (clause 5.8.1.4).

use sea_query::Iden;

/// The tenant-scoped `subscription_companions` table: a local subscription and
/// the companion registration subscription mirroring it.
#[derive(Iden, Clone, Copy)]
pub enum SubscriptionCompanions {
    /// Table name: `subscription_companions`
    Table,
    /// The local entity subscription.
    LocalSubscriptionId,
    /// The minted companion registration subscription.
    CompanionSubscriptionId,
}

/// The tenant-scoped `forwarded_subscriptions` table: the reduced copy of a
/// local subscription placed at one registration's context source.
#[derive(Iden, Clone, Copy)]
pub enum ForwardedSubscriptions {
    /// Table name: `forwarded_subscriptions`
    Table,
    /// The local entity subscription.
    LocalSubscriptionId,
    /// The registration whose source holds the copy.
    RegistrationId,
    /// The identifier of the copy at the source.
    RemoteSubscriptionId,
}

/// The platform `forwarded_subscription_routes` table: every minted
/// subscription identifier and the tenant it belongs to.
#[derive(Iden, Clone, Copy)]
pub enum ForwardedSubscriptionRoutes {
    /// Table name: `forwarded_subscription_routes`
    Table,
    /// The minted subscription identifier.
    MintedSubscriptionId,
    /// The owning tenant, `NULL` for the unnamed default tenant.
    Tenant,
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::{ForwardedSubscriptionRoutes, ForwardedSubscriptions, SubscriptionCompanions};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn relation_and_column_names() {
        assert_eq!(SubscriptionCompanions::Table.to_string(), "subscription_companions");
        assert_eq!(SubscriptionCompanions::CompanionSubscriptionId.to_string(), "companion_subscription_id");
        assert_eq!(ForwardedSubscriptions::Table.to_string(), "forwarded_subscriptions");
        assert_eq!(ForwardedSubscriptions::RemoteSubscriptionId.to_string(), "remote_subscription_id");
        assert_eq!(ForwardedSubscriptionRoutes::Table.to_string(), "forwarded_subscription_routes");
        assert_eq!(ForwardedSubscriptionRoutes::MintedSubscriptionId.to_string(), "minted_subscription_id");
    }
}
