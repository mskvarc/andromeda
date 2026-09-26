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

use crate::{
    EntityService,
    error::EntityError,
    notification_fanout::{ChangedEntity, MutationSnapshots},
};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{
        attribute_change::{AttributeChange, classify_attribute_changes},
        attribute_expiry::{attribute_stands, instance_expired},
    },
    notification::{ChangeOrigin, NotificationTrigger},
};
use chrono::{DateTime, Utc};
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
    pub(crate) const fn entity(trigger: NotificationTrigger, attributes: Vec<AttributeName>) -> NotificationGroup {
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
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when either state carries an instance
/// whose `datasetId` cannot be read: the stored state was admitted when it was
/// written, and the new state when its request was.
pub(crate) fn attribute_change_groups(previous: &Value, current: &Value) -> Result<Vec<NotificationGroup>, EntityError> {
    Ok(classify_attribute_changes(previous, current)
        .map_err(EntityError::internal)?
        .into_iter()
        .map(NotificationGroup::attribute)
        .collect())
}

/// The attributes a group still reports once the instances expired at `at`
/// are treated as absent, or `None` when the one instance it reports is itself
/// expired.
///
/// Clause 4.22 makes an instance past its `expiresAt` invalid, so writing one
/// changes nothing a subscriber can see: an attribute-instance group whose
/// instance has expired in the entity the write left behind is dropped, and an
/// entity-level group reports only the attributes that still have a standing
/// instance, which is what `watchedAttributes` is matched against. A deletion
/// group reports what the mutation removed and is kept as it is -- the removal
/// of an expired instance by the expiry sweep is still an `attributeDeleted`.
fn standing_attributes(group: &NotificationGroup, current: Option<&Value>, at: DateTime<Utc>) -> Option<Vec<AttributeName>> {
    let Some(current) = current else {
        return Some(group.attributes.clone()); // clone: the groups are borrowed and each event owns its names
    };
    match group.trigger {
        NotificationTrigger::AttributeDeleted | NotificationTrigger::EntityDeleted => Some(group.attributes.clone()), // clone: same
        NotificationTrigger::AttributeCreated | NotificationTrigger::AttributeUpdated => {
            let expired = group
                .attributes
                .iter()
                .any(|attribute| instance_expired(current, attribute.as_str(), group.dataset_id.as_ref(), at));
            (!expired).then(|| group.attributes.clone()) // clone: same
        }
        NotificationTrigger::EntityCreated | NotificationTrigger::EntityUpdated => Some(
            group
                .attributes
                .iter()
                .filter(|attribute| attribute_stands(current, attribute.as_str(), at))
                .cloned() // clone: each event owns its names
                .collect(),
        ),
    }
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
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        groups: &[NotificationGroup],
        entity_snapshot: Option<&Arc<Value>>,
        previous_snapshot: Option<&Value>,
    ) -> NotificationPlan {
        let snapshot_ref = entity_snapshot.map(|snapshot| &**snapshot);
        let mutated = ChangedEntity {
            target,
            id: entity_id,
            types: entity_types,
        };
        let snapshots = MutationSnapshots {
            current: snapshot_ref,
            previous: previous_snapshot,
        };
        let mut matches = Vec::new();
        let mut events = Vec::with_capacity(groups.len());
        let now = Utc::now();
        for group in groups {
            let Some(attributes) = standing_attributes(group, snapshot_ref, now) else {
                continue;
            };
            matches.extend(self.build_subscription_matches_for_instance(mutated, group.trigger, &attributes, group.dataset_id.as_ref(), snapshots));
            events.push(
                ChangeEvent::builder()
                    .entity_id(entity_id.clone()) // clone: the event owns its identifier
                    .entity_types(entity_types.to_vec())
                    .trigger_reason(group.trigger)
                    .occurred_at(Utc::now())
                    .origin(ChangeOrigin::Local)
                    .changed_attributes(attributes)
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
    use crate::notification_plan::{NotificationGroup, attribute_change_groups, standing_attributes};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
    }

    /// An entity whose `speed` expired in 2000 and whose `heading` never expires.
    fn half_expired() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2000-01-01T00:00:00Z"}]
            }],
            "https://example.com/heading": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]
            }]
        }])
    }

    fn attribute_group(trigger: NotificationTrigger, attribute: &str) -> NotificationGroup {
        NotificationGroup {
            trigger,
            attributes: vec![name(attribute)],
            dataset_id: None,
        }
    }

    /// Clause 4.22: writing an instance already past its `expiresAt` changes
    /// nothing a subscriber can see, so its attribute-instance change is not
    /// reported to `watchedAttributes` at all.
    #[test]
    fn an_attribute_change_to_an_expired_instance_is_not_reported() {
        let at = instant("2030-01-01T00:00:00Z");
        let current = half_expired();
        pretty_assertions::assert_eq!(
            standing_attributes(
                &attribute_group(NotificationTrigger::AttributeUpdated, "https://example.com/speed"),
                Some(&current),
                at
            ),
            None
        );
        pretty_assertions::assert_eq!(
            standing_attributes(
                &attribute_group(NotificationTrigger::AttributeUpdated, "https://example.com/heading"),
                Some(&current),
                at
            ),
            Some(vec![name("https://example.com/heading")])
        );
    }

    #[test]
    fn an_entity_level_change_reports_only_the_attributes_that_still_stand() {
        let group = NotificationGroup::entity(
            NotificationTrigger::EntityCreated,
            vec![name("https://example.com/speed"), name("https://example.com/heading")],
        );
        pretty_assertions::assert_eq!(
            standing_attributes(&group, Some(&half_expired()), instant("2030-01-01T00:00:00Z")),
            Some(vec![name("https://example.com/heading")])
        );
    }

    /// The expiry sweep removing an expired instance is still an
    /// `attributeDeleted`: a deletion reports what the mutation removed.
    #[test]
    fn a_deletion_is_reported_whatever_the_removed_instance_carried() {
        let group = attribute_group(NotificationTrigger::AttributeDeleted, "https://example.com/speed");
        pretty_assertions::assert_eq!(
            standing_attributes(&group, Some(&half_expired()), instant("2030-01-01T00:00:00Z")),
            Some(vec![name("https://example.com/speed")])
        );
    }

    #[test]
    fn a_pure_addition_raises_only_attribute_created() {
        let before =
            json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]});
        let after = json!({"https://example.com/a": before["https://example.com/a"], "https://example.com/b": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]});
        let groups = attribute_change_groups(&before, &after).expect("readable datasetIds");
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
        let groups = attribute_change_groups(&before, &after).expect("readable datasetIds");
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeUpdated);
    }

    #[test]
    fn a_removed_attribute_raises_one_attribute_deleted_each() {
        let before = json!({"https://example.com/a": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}], "https://example.com/b": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}], "https://example.com/c": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let after = json!({"https://example.com/a": before["https://example.com/a"]});
        let groups = attribute_change_groups(&before, &after).expect("readable datasetIds");
        pretty_assertions::assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.trigger == NotificationTrigger::AttributeDeleted));
        assert!(groups.iter().all(|group| group.attributes.len() == 1));
    }

    #[test]
    fn a_mixed_change_raises_one_precise_event_for_each_transition() {
        let before = json!({"https://example.com/keep": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}], "https://example.com/drop": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let after = json!({"https://example.com/keep": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}], "https://example.com/new": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]});
        let groups = attribute_change_groups(&before, &after).expect("readable datasetIds");
        pretty_assertions::assert_eq!(groups.len(), 3);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeDeleted);
        pretty_assertions::assert_eq!(groups[0].attributes[0].as_str(), "https://example.com/drop");
        pretty_assertions::assert_eq!(groups[1].trigger, NotificationTrigger::AttributeUpdated);
        pretty_assertions::assert_eq!(groups[1].attributes[0].as_str(), "https://example.com/keep");
        pretty_assertions::assert_eq!(groups[2].trigger, NotificationTrigger::AttributeCreated);
        pretty_assertions::assert_eq!(groups[2].attributes[0].as_str(), "https://example.com/new");
    }
}
