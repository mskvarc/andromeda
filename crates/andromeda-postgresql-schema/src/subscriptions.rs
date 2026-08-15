//! The `subscriptions` relation.
//!
//! A subscription carries two identities: `id`, the external NGSI-LD URI a
//! client registers and matches under, and `subscription_key`, the internal
//! surrogate the notification queue references so a delivery survives the
//! external id being reused. Both entity and context-source subscriptions share
//! this one table, distinguished by `kind`.

use sea_query::Iden;

/// The `subscriptions` table - registered NGSI-LD subscriptions.
#[derive(Iden, Clone, Copy)]
pub enum Subscriptions {
    /// Table name: `subscriptions`
    Table,
    /// External NGSI-LD subscription URI a client registers and matches under
    Id,
    /// Internal surrogate the notification queue references
    SubscriptionKey,
    /// Which subscription family a row is: `entity` or `csource`
    Kind,
    /// The NGSI-LD resource type token, always `Subscription`
    Type,
    /// The `subscriptionName` member (clause 5.2.12)
    SubscriptionName,
    /// The `description` member
    Description,
    /// The watched `EntityInfo` selectors, as JSONB
    Entities,
    /// The `watchedAttributes` IRIs
    WatchedAttributes,
    /// The `notificationTrigger` change types
    NotificationTrigger,
    /// The `timeInterval` period in seconds, when periodic
    TimeInterval,
    /// The `q` query filter
    Q,
    /// The `geoQ` geo-query filter, as JSONB
    GeoQ,
    /// The `scopeQ` scope filter
    ScopeQ,
    /// The `csf` context-source filter
    Csf,
    /// The `datasetId` selectors an entity subscription narrows to
    DatasetId,
    /// Whether the subscription is active
    IsActive,
    /// The `notification` parameters, as JSONB
    Notification,
    /// The `expiresAt` instant
    ExpiresAt,
    /// The `throttling` minimum interval between notifications
    Throttling,
    /// The subscription status enum (`active`, `paused`, `expired`)
    Status,
    /// The `temporalQ` temporal-query filter, as JSONB
    TemporalQ,
    /// The `lang` language filter
    Lang,
    /// The `jsonldContext` the subscription was registered under
    JsonldContext,
    /// Whether the subscription is confined to the local broker (`local`)
    LocalOnly,
    /// How many notifications have been sent (`notification.timesSent`)
    TimesSent,
    /// The instant of the last notification attempt
    LastNotification,
    /// The instant of the last successful notification
    LastSuccess,
    /// The instant of the last failed notification
    LastFailure,
    /// The instant the subscription was created (clause 4.8 `createdAt`)
    CreatedAt,
    /// The instant the subscription was last modified (clause 4.8 `modifiedAt`)
    ModifiedAt,
}

#[cfg(test)]
mod tests {
    use crate::subscriptions::Subscriptions;
    use sea_query::Iden;

    #[test]
    fn subscriptions_table_and_columns() {
        assert_eq!(Subscriptions::Table.to_string(), "subscriptions");
        assert_eq!(Subscriptions::Id.to_string(), "id");
        assert_eq!(Subscriptions::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(Subscriptions::Kind.to_string(), "kind");
        assert_eq!(Subscriptions::WatchedAttributes.to_string(), "watched_attributes");
        assert_eq!(Subscriptions::NotificationTrigger.to_string(), "notification_trigger");
        assert_eq!(Subscriptions::TimeInterval.to_string(), "time_interval");
        assert_eq!(Subscriptions::JsonldContext.to_string(), "jsonld_context");
        assert_eq!(Subscriptions::LocalOnly.to_string(), "local_only");
        assert_eq!(Subscriptions::LastNotification.to_string(), "last_notification");
    }
}
