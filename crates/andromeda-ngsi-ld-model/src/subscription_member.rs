//! The members of a Subscription that clause 5.5.8 lets an update delete.
//!
//! Table 5.2.12-1 gives each of them cardinality `0..1`. Clause 5.5.8 gives an
//! NGSI-LD Null written into one the meaning "the target member is deleted" in
//! an update, and clause 5.2.1 makes it `BadRequestData` anywhere else, a
//! create among them: "None of the members described admit a null value
//! directly".

use strum::Display;

/// A member of a Subscription an update may delete (Table 5.2.12-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum SubscriptionMember {
    /// `subscriptionName`, the human-readable name.
    SubscriptionName,
    /// `description`, the human-readable description.
    Description,
    /// `entities`, the entity selectors.
    Entities,
    /// `watchedAttributes`, the attribute names a subscription watches, which
    /// are URIs once expanded.
    WatchedAttributes,
    /// `notificationTrigger`, the changes that trigger a notification.
    NotificationTrigger,
    /// `timeInterval`, the period of a periodic subscription.
    TimeInterval,
    /// `q`, the query selecting entities.
    Q,
    /// `geoQ`, the geo-query selecting entities.
    GeoQ,
    /// `scopeQ`, the scope query selecting entities.
    ScopeQ,
    /// `csf`, the context source filter.
    Csf,
    /// `datasetId`, the dataset identifiers a notification carries.
    DatasetId,
    /// `temporalQ`, the temporal query.
    TemporalQ,
    /// `expandValues`, the attributes whose values are expanded.
    ExpandValues,
    /// `jsonKeys`, the attributes whose values are opaque JSON.
    JsonKeys,
    /// `throttling`, the minimum interval between notifications.
    Throttling,
    /// `expiresAt`, the instant the subscription expires.
    ExpiresAt,
    /// `isActive`, whether the subscription is active.
    IsActive,
    /// `lang`, the preferred language.
    Lang,
    /// `jsonldContext`, the `@context` notifications are rendered with.
    JsonldContext,
    /// `localOnly`, whether matching is restricted to local entities.
    LocalOnly,
    /// `splitEntities`, whether entities are split across context sources.
    SplitEntities,
    /// `ngsildConformance`, the specification version notifications conform to.
    NgsildConformance,
}

#[cfg(test)]
mod tests {
    use crate::subscription_member::SubscriptionMember;

    /// The members are named as Table 5.2.12-1 spells them.
    #[test]
    fn a_member_is_named_as_the_specification_spells_it() {
        pretty_assertions::assert_eq!(SubscriptionMember::JsonldContext.to_string(), "jsonldContext");
        pretty_assertions::assert_eq!(SubscriptionMember::DatasetId.to_string(), "datasetId");
        pretty_assertions::assert_eq!(SubscriptionMember::WatchedAttributes.to_string(), "watchedAttributes");
        pretty_assertions::assert_eq!(SubscriptionMember::Q.to_string(), "q");
        pretty_assertions::assert_eq!(SubscriptionMember::GeoQ.to_string(), "geoQ");
        pretty_assertions::assert_eq!(SubscriptionMember::NgsildConformance.to_string(), "ngsildConformance");
    }
}
