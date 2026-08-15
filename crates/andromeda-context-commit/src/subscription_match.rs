//! One subscription a committing context write owes a notification.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Everything a notification event needs about one subscription that matched.
#[derive(Debug, TypedBuilder)]
pub struct SubscriptionMatch {
    /// Subscription that matched the entity change.
    pub subscription_id: Uri,
    /// Entity that triggered the notification.
    pub entity_id: Uri,
    /// Mutation trigger reason.
    pub trigger: NotificationTrigger,
    /// Expanded entity snapshot after the mutation.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Value>,
    /// Expanded entity snapshot before the mutation.
    #[builder(default = Default::default())]
    pub previous_snapshot: Option<Value>,
    /// Attributes changed by the mutation.
    #[builder(default = Default::default())]
    pub changed_attrs: Vec<AttributeName>,
}

#[cfg(test)]
mod tests {
    use crate::subscription_match::SubscriptionMatch;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;

    #[test]
    fn the_builder_defaults_optional_notification_data() {
        let entry = SubscriptionMatch::builder()
            .subscription_id("urn:ngsi-ld:Subscription:1".parse().expect("valid subscription URI"))
            .entity_id("urn:ngsi-ld:A".parse().expect("valid entity URI"))
            .trigger(NotificationTrigger::EntityUpdated)
            .build();
        assert!(entry.entity_snapshot.is_none());
        assert!(entry.previous_snapshot.is_none());
        assert!(entry.changed_attrs.is_empty());
    }
}
