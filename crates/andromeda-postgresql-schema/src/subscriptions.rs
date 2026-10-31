//! The `subscriptions` relation.
//!
//! A subscription carries two identities: `id`, the external NGSI-LD URI a
//! client registers and matches under, and `subscription_key`, the internal
//! surrogate the notification queue references so a delivery survives the
//! external id being reused. Both entity and context-source subscriptions share
//! this one table, distinguished by `kind`. The delivery counters a
//! notification moves on every send live apart in `subscription_delivery_state`
//! so that a delivery never rewrites the wide subscription row.

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
    /// Which subscription family a row is (`SubscriptionKindCode`)
    Kind,
    /// The `context_source_change_clock` revision a csource subscription's latest client write drew
    ContextSourceRevision,
    /// The `context_source_revision` whose notification a broker has claimed
    EvaluatedContextSourceRevision,
    /// The `subscriptionName` member (clause 5.2.12)
    SubscriptionName,
    /// The `description` member
    Description,
    /// The watched `EntityInfo` selectors, as JSON
    Entities,
    /// The `attribute_names` keys of the `watchedAttributes` IRIs
    WatchedAttributes,
    /// The `notificationTrigger` change types
    NotificationTrigger,
    /// The `timeInterval` period in seconds, when periodic
    TimeInterval,
    /// The `q` query filter
    Q,
    /// The `q` query filter with its trailing members as the subscriber wrote them
    QWrittenMembers,
    /// The `geoQ` geo-query filter, as JSON
    GeoQ,
    /// The `scopeQ` scope filter
    ScopeQ,
    /// The `csf` context-source filter
    Csf,
    /// The `attribute_names` keys of the `expandValues` names whose `q` operands were coerced (clause 5.2.12)
    ExpandValues,
    /// The `attribute_names` keys of the `jsonKeys` names whose `q` object keys were expanded (clause 5.2.12)
    JsonKeys,
    /// The `datasetId` selectors an entity subscription narrows to
    DatasetId,
    /// The `notification` parameters, as JSON
    Notification,
    /// The `expiresAt` instant
    ExpiresAt,
    /// The `throttling` minimum interval between notifications
    Throttling,
    /// The subscription status code (`SubscriptionStatusCode`)
    Status,
    /// The `temporalQ` temporal-query filter, as JSON
    TemporalQ,
    /// The `lang` language filter
    Lang,
    /// The `jsonldContext` the subscription was registered under
    JsonldContext,
    /// Whether the subscription is confined to the local broker (`local`)
    LocalOnly,
    /// Whether entities are assumed split across sources (`splitEntities`), `NULL` deferring to the deployment default
    SplitEntities,
    /// The `ngsildConformance` version notifications are amended to (clause 5.2.12), as `major.minor` text
    NgsildConformance,
    /// The instant the subscription was created (clause 4.8 `createdAt`)
    CreatedAt,
    /// The instant the subscription was last modified (clause 4.8 `modifiedAt`)
    ModifiedAt,
    /// The transaction timestamp at which the subscription was created active or last set active again
    ActiveSince,
    /// The transaction timestamp at which the subscription was created or what it selects last changed
    SelectedSince,
}

/// The `subscription_delivery_state` table - the per-subscription delivery
/// counters of clauses 5.2.14.2 and 5.2.15, one row per subscription, moved by
/// every notification sent.
///
/// The row is narrow and carries no index but its primary key, so the claim
/// and the success/failure marks are heap-only-tuple updates that never touch
/// the subscription row itself.
#[derive(Iden, Clone, Copy)]
pub enum SubscriptionDeliveryState {
    /// Table name: `subscription_delivery_state`
    Table,
    /// Foreign key to `subscriptions`, cascading on deletion; also the primary key
    SubscriptionKey,
    /// How many notifications have been sent (`notification.timesSent`)
    TimesSent,
    /// How many deliveries drew an unsuccessful response or timed out (`notification.timesFailed`)
    TimesFailed,
    /// The instant of the last notification attempt (`notification.lastNotification`)
    LastNotification,
    /// The instant of the last successful notification (`notification.lastSuccess`)
    LastSuccess,
    /// The instant of the last failed notification (`notification.lastFailure`)
    LastFailure,
    /// The sweep instant at which a `timeInterval` subscription last answered
    /// its grid, by a notification or by finding nothing to notify; NULL until
    /// it first does. Not a spec member.
    LastIntervalTick,
}

/// The live-only `subscription_notified_registrations` table: the context
/// source registrations each context source registration subscription was
/// notified of as matching.
#[derive(Iden, Clone, Copy)]
pub enum SubscriptionNotifiedRegistrations {
    /// Table name: `subscription_notified_registrations`
    Table,
    /// The subscription notified.
    SubscriptionKey,
    /// The registration it was notified of.
    RegistrationId,
    /// The latest registration change revision that claimed an `updated`
    /// notification for the pair; NULL until one does.
    ReportedRevision,
}

#[cfg(test)]
mod tests {
    use crate::subscriptions::{SubscriptionDeliveryState, SubscriptionNotifiedRegistrations, Subscriptions};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn notified_registrations_table_and_columns() {
        assert_eq!(SubscriptionNotifiedRegistrations::Table.to_string(), "subscription_notified_registrations");
        assert_eq!(SubscriptionNotifiedRegistrations::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(SubscriptionNotifiedRegistrations::RegistrationId.to_string(), "registration_id");
        assert_eq!(SubscriptionNotifiedRegistrations::ReportedRevision.to_string(), "reported_revision");
    }

    #[test]
    fn subscriptions_table_and_columns() {
        assert_eq!(Subscriptions::Table.to_string(), "subscriptions");
        assert_eq!(Subscriptions::Id.to_string(), "id");
        assert_eq!(Subscriptions::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(Subscriptions::Kind.to_string(), "kind");
        assert_eq!(Subscriptions::ContextSourceRevision.to_string(), "context_source_revision");
        assert_eq!(Subscriptions::EvaluatedContextSourceRevision.to_string(), "evaluated_context_source_revision");
        assert_eq!(Subscriptions::WatchedAttributes.to_string(), "watched_attributes");
        assert_eq!(Subscriptions::ExpandValues.to_string(), "expand_values");
        assert_eq!(Subscriptions::JsonKeys.to_string(), "json_keys");
        assert_eq!(Subscriptions::QWrittenMembers.to_string(), "q_written_members");
        assert_eq!(Subscriptions::NotificationTrigger.to_string(), "notification_trigger");
        assert_eq!(Subscriptions::TimeInterval.to_string(), "time_interval");
        assert_eq!(Subscriptions::JsonldContext.to_string(), "jsonld_context");
        assert_eq!(Subscriptions::LocalOnly.to_string(), "local_only");
        assert_eq!(Subscriptions::SplitEntities.to_string(), "split_entities");
        assert_eq!(Subscriptions::NgsildConformance.to_string(), "ngsild_conformance");
        assert_eq!(Subscriptions::ModifiedAt.to_string(), "modified_at");
        assert_eq!(Subscriptions::ActiveSince.to_string(), "active_since");
        assert_eq!(Subscriptions::SelectedSince.to_string(), "selected_since");
    }

    #[test]
    fn subscription_delivery_state_table_and_columns() {
        assert_eq!(SubscriptionDeliveryState::Table.to_string(), "subscription_delivery_state");
        assert_eq!(SubscriptionDeliveryState::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(SubscriptionDeliveryState::TimesSent.to_string(), "times_sent");
        assert_eq!(SubscriptionDeliveryState::TimesFailed.to_string(), "times_failed");
        assert_eq!(SubscriptionDeliveryState::LastNotification.to_string(), "last_notification");
        assert_eq!(SubscriptionDeliveryState::LastSuccess.to_string(), "last_success");
        assert_eq!(SubscriptionDeliveryState::LastFailure.to_string(), "last_failure");
        assert_eq!(SubscriptionDeliveryState::LastIntervalTick.to_string(), "last_interval_tick");
    }
}
