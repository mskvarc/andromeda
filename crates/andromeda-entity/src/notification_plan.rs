//! Planning the notifications one mutation owes, on both channels at once.
//!
//! A mutation raises the same change on two channels: the durable
//! [`SubscriptionMatch`] records committed in the mutation's transaction, and
//! the in-process [`ChangeEvent`] stream. Both describe the change with the same
//! per-attribute triggers (clause 5.2.12), so they are planned together here
//! rather than derived twice and risking divergence.
//!
//! A single mutation can change several attribute instances, so the plan is
//! expressed as a list of [`NotificationGroup`]s, one per change, each
//! identifying one attribute instance or one entity lifecycle event.

use crate::EntityService;
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::attribute_change::{AttributeChange, classify_attribute_changes},
    notification::{ChangeOrigin, NotificationTrigger},
};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

/// One trigger a mutation raises.
pub(crate) struct NotificationGroup {
    /// The change this group describes to a subscriber (clause 5.2.12).
    pub trigger: NotificationTrigger,
    /// Attribute names used for `watchedAttributes` matching.
    pub attributes: Vec<AttributeName>,
    /// Dataset identity for a precise attribute-instance change.
    pub dataset_id: Option<Uri>,
}

impl NotificationGroup {
    /// Construct one precise attribute-instance notification group.
    pub(crate) fn attribute(change: AttributeChange) -> NotificationGroup {
        NotificationGroup {
            trigger: change.kind.notification_trigger(),
            attributes: vec![change.key.attr],
            dataset_id: change.key.dataset_id,
        }
    }

    /// Construct an entity lifecycle group, carrying all attribute names for
    /// `watchedAttributes` gating.
    pub(crate) fn entity(trigger: NotificationTrigger, attributes: Vec<AttributeName>) -> NotificationGroup {
        NotificationGroup {
            trigger,
            attributes,
            dataset_id: None,
        }
    }
}

/// Both notification channels planned from one mutation.
pub(crate) struct NotificationPlan {
    /// Durable records committed in the mutation's transaction, one per matched
    /// subscription.
    pub matches: Vec<SubscriptionMatch>,
    /// In-process change events, one per precise change.
    pub events: Vec<ChangeEvent>,
}

/// Classify a before/after Entity pair into precise attribute-instance changes
/// under clause 5.2.12.
pub(crate) fn attribute_change_groups(previous: &Value, current: &Value) -> Vec<NotificationGroup> {
    classify_attribute_changes(previous, current)
        .into_iter()
        .map(NotificationGroup::attribute)
        .collect()
}

impl EntityService {
    /// Plan both notification channels for one mutation from its per-trigger
    /// groups.
    ///
    /// `entity_snapshot` is the entity as it stands after the mutation — the
    /// payload a `keyValues`/`normalized` notification renders, never the
    /// request fragment. Create/update filters evaluate this state; deletion
    /// filters evaluate `previous_snapshot`. The previous state is also carried
    /// for `showChanges`.
    pub(crate) fn plan_notifications(
        &self,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        groups: &[NotificationGroup],
        entity_snapshot: Option<&Arc<Value>>,
        previous_snapshot: Option<&Value>,
    ) -> NotificationPlan {
        let snapshot_ref = entity_snapshot.map(|snapshot| &**snapshot);
        let mut matches = Vec::new();
        let mut events = Vec::with_capacity(groups.len());
        for group in groups {
            matches.extend(self.build_subscription_matches_for_instance(
                entity_id,
                entity_types,
                group.trigger,
                &group.attributes,
                group.dataset_id.as_ref(),
                snapshot_ref,
                previous_snapshot,
            ));
            events.push(
                ChangeEvent::builder()
                    .entity_id(entity_id.clone()) // clone: the event owns its identifier
                    .entity_types(entity_types.to_vec())
                    .trigger_reason(group.trigger)
                    .occurred_at(Utc::now())
                    .origin(ChangeOrigin::Local)
                    .changed_attributes(group.attributes.clone()) // clone: the event owns its names
                    .changed_dataset_id(group.dataset_id.clone()) // clone: the event owns its dataset identity
                    .previous_attributes(previous_snapshot.cloned())
                    .current_payload(entity_snapshot.map(Arc::clone)) // clone: Arc ref-count bump
                    .build(),
            );
        }
        NotificationPlan { matches, events }
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_plan::attribute_change_groups;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use serde_json::json;

    #[test]
    fn a_pure_addition_raises_only_attribute_created() {
        let before =
            json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]});
        let after = json!({"https://example.com/a": before["https://example.com/a"], "https://example.com/b": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]});
        let groups = attribute_change_groups(&before, &after);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeCreated);
        pretty_assertions::assert_eq!(groups[0].attributes[0].as_str(), "https://example.com/b");
    }

    #[test]
    fn an_overwrite_of_an_existing_attribute_raises_only_attribute_updated() {
        let before =
            json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]});
        let after =
            json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]});
        let groups = attribute_change_groups(&before, &after);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeUpdated);
    }

    #[test]
    fn a_removed_attribute_raises_one_attribute_deleted_each() {
        let before = json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}], "https://example.com/b": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}], "https://example.com/c": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let after = json!({"https://example.com/a": before["https://example.com/a"]});
        let groups = attribute_change_groups(&before, &after);
        pretty_assertions::assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.trigger == NotificationTrigger::AttributeDeleted));
        assert!(groups.iter().all(|group| group.attributes.len() == 1));
    }

    #[test]
    fn a_mixed_change_raises_one_precise_event_for_each_transition() {
        let before = json!({"https://example.com/keep": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}], "https://example.com/drop": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let after = json!({"https://example.com/keep": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}], "https://example.com/new": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let groups = attribute_change_groups(&before, &after);
        pretty_assertions::assert_eq!(groups.len(), 3);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeDeleted);
        pretty_assertions::assert_eq!(groups[0].attributes[0].as_str(), "https://example.com/drop");
        pretty_assertions::assert_eq!(groups[1].trigger, NotificationTrigger::AttributeUpdated);
        pretty_assertions::assert_eq!(groups[1].attributes[0].as_str(), "https://example.com/keep");
        pretty_assertions::assert_eq!(groups[2].trigger, NotificationTrigger::AttributeCreated);
        pretty_assertions::assert_eq!(groups[2].attributes[0].as_str(), "https://example.com/new");
    }
}
