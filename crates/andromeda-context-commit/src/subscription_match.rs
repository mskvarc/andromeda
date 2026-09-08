//! One subscription a committing context write owes a notification.

use crate::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Everything a notification event needs about one subscription that matched.
#[derive(Debug, TypedBuilder)]
pub struct SubscriptionMatch {
    /// Subscription that matched the entity change, by the surrogate key the
    /// notification queue references.
    ///
    /// The key rather than the URI: the queue row this match becomes carries the
    /// key, so handing it over resolved lets the delivery insert write every
    /// matched subscription of a write in one statement without a lookup.
    pub subscription_key: SubscriptionKey,
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
    /// Dataset discriminator of the changed Attribute instance.
    #[builder(default = Default::default())]
    pub changed_dataset_id: Option<Uri>,
}

#[cfg(test)]
mod tests {
    use crate::{subscription_key::SubscriptionKey, subscription_match::SubscriptionMatch};
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;

    #[test]
    fn the_builder_defaults_optional_notification_data() {
        let entry = SubscriptionMatch::builder()
            .subscription_key(SubscriptionKey::try_from(1).expect("a positive key"))
            .entity_id("urn:ngsi-ld:A".parse().expect("valid entity URI"))
            .trigger(NotificationTrigger::EntityUpdated)
            .build();
        assert!(entry.entity_snapshot.is_none());
        assert!(entry.previous_snapshot.is_none());
        assert_eq!(entry.changed_attrs, [] as [andromeda_ngsi_ld_lexicon::attribute_name::AttributeName; 0]);
    }
}
