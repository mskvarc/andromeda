//! One subscription a committing context write owes a notification.

use crate::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::EntityChange;
use serde_json::Value;
use std::sync::Arc;
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
    pub entity_id: Identifier,
    /// What the write did to the Entity, which the notification reports.
    pub change: EntityChange,
    /// Expanded entity snapshot after the mutation, shared by every match of
    /// the same change.
    #[builder(default = Default::default())]
    pub entity_snapshot: Option<Arc<Value>>,
    /// Expanded entity snapshot before the mutation, shared by every match of
    /// the same change.
    #[builder(default = Default::default())]
    pub previous_snapshot: Option<Arc<Value>>,
}

#[cfg(test)]
mod tests {
    use crate::{subscription_key::SubscriptionKey, subscription_match::SubscriptionMatch};
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};

    #[test]
    fn the_builder_defaults_optional_notification_data() {
        let entry = SubscriptionMatch::builder()
            .subscription_key(SubscriptionKey::try_from(1).expect("a positive key"))
            .entity_id("urn:ngsi-ld:A".parse().expect("valid entity URI"))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .build();
        assert!(entry.entity_snapshot.is_none());
        assert!(entry.previous_snapshot.is_none());
    }
}
