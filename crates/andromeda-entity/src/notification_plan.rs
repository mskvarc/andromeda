//! Planning the notifications one mutation owes, on both channels at once.
//!
//! A mutation raises the same change on two channels: the durable
//! [`SubscriptionMatch`] records committed in the mutation's transaction, and
//! the in-process [`ChangeEvent`] stream. Both describe the change with the same
//! per-attribute triggers (clause 5.2.12), so they are planned together here
//! rather than derived twice and risking divergence.
//!
//! A single mutation can raise several triggers at once — an attribute update
//! that also removes an attribute reports both an `attributeUpdated` and an
//! `attributeDeleted` (clause 5.8.3) — so the plan is expressed as a list of
//! [`NotificationGroup`]s, one per trigger, each naming only the attributes that
//! trigger concerns.

use crate::EntityService;
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use chrono::Utc;
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

/// One trigger a mutation raises, and the attributes that trigger concerns.
///
/// `attributes` is empty for whole-entity triggers (`entityDeleted`) and carries
/// exactly the changed names for attribute-level ones.
pub(crate) struct NotificationGroup {
    /// The change this group describes to a subscriber (clause 5.2.12).
    pub trigger: NotificationTrigger,
    /// The attribute names this trigger concerns.
    pub attributes: Vec<AttributeName>,
}

/// Both notification channels planned from one mutation.
pub(crate) struct NotificationPlan {
    /// Durable records committed in the mutation's transaction, one per matched
    /// subscription.
    pub matches: Vec<SubscriptionMatch>,
    /// In-process change events, one per trigger the mutation raised.
    pub events: Vec<ChangeEvent>,
}

/// Partition an attribute-level change into its per-trigger groups (clause
/// 5.6.2 semantics, reported under clause 5.2.12).
///
/// A removed attribute raises one `attributeDeleted` each; the names the request
/// carried split into `attributeCreated` for those the entity did not already
/// hold and `attributeUpdated` for those it did. Empty groups are omitted so a
/// mutation that only adds attributes raises no spurious update.
pub(crate) fn attribute_change_groups(
    attrs_before: &HashSet<AttributeName>,
    attrs_after: &HashSet<AttributeName>,
    all_changed: &[AttributeName],
) -> Vec<NotificationGroup> {
    let mut groups = Vec::new();
    for deleted in attrs_before.difference(attrs_after) {
        groups.push(NotificationGroup {
            trigger: NotificationTrigger::AttributeDeleted,
            attributes: vec![deleted.clone()], // clone: the group owns the removed name
        });
    }
    // clone: owned names from filtered references
    let created: Vec<AttributeName> = all_changed.iter().filter(|name| !attrs_before.contains(name.as_str())).cloned().collect();
    if !created.is_empty() {
        groups.push(NotificationGroup {
            trigger: NotificationTrigger::AttributeCreated,
            attributes: created,
        });
    }
    // clone: owned names from filtered references
    let updated: Vec<AttributeName> = all_changed.iter().filter(|name| attrs_before.contains(name.as_str())).cloned().collect();
    if !updated.is_empty() {
        groups.push(NotificationGroup {
            trigger: NotificationTrigger::AttributeUpdated,
            attributes: updated,
        });
    }
    groups
}

impl EntityService {
    /// Plan both notification channels for one mutation from its per-trigger
    /// groups.
    ///
    /// `entity_snapshot` is the entity as it stands after the mutation — the
    /// payload a `keyValues`/`normalized` notification renders and the matcher's
    /// entity-level filters (`q`, `geoQ`) evaluate against — never the request
    /// fragment. `previous_snapshot` is the state before the mutation, carried
    /// only when a `showChanges` subscriber needs the diff.
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
            matches.extend(self.build_subscription_matches(entity_id, entity_types, group.trigger, &group.attributes, snapshot_ref, previous_snapshot));
            events.push(
                ChangeEvent::builder()
                    .entity_id(entity_id.clone()) // clone: the event owns its identifier
                    .entity_types(entity_types.to_vec())
                    .trigger_reason(group.trigger)
                    .occurred_at(Utc::now())
                    .origin(ChangeOrigin::Local)
                    .changed_attributes(group.attributes.clone()) // clone: the event owns its names
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
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use std::collections::HashSet;

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn set(values: &[&str]) -> HashSet<AttributeName> {
        values.iter().map(|value| name(value)).collect()
    }

    #[test]
    fn a_pure_addition_raises_only_attribute_created() {
        let before = set(&["https://example.com/a"]);
        let after = set(&["https://example.com/a", "https://example.com/b"]);
        let groups = attribute_change_groups(&before, &after, &[name("https://example.com/b")]);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeCreated);
        pretty_assertions::assert_eq!(groups[0].attributes, vec![name("https://example.com/b")]);
    }

    #[test]
    fn an_overwrite_of_an_existing_attribute_raises_only_attribute_updated() {
        let before = set(&["https://example.com/a"]);
        let after = set(&["https://example.com/a"]);
        let groups = attribute_change_groups(&before, &after, &[name("https://example.com/a")]);
        pretty_assertions::assert_eq!(groups.len(), 1);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeUpdated);
    }

    #[test]
    fn a_removed_attribute_raises_one_attribute_deleted_each() {
        let before = set(&["https://example.com/a", "https://example.com/b", "https://example.com/c"]);
        let after = set(&["https://example.com/a"]);
        // No request names change; the removals are inferred from the diff.
        let groups = attribute_change_groups(&before, &after, &[]);
        pretty_assertions::assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.trigger == NotificationTrigger::AttributeDeleted));
        assert!(groups.iter().all(|group| group.attributes.len() == 1));
    }

    #[test]
    fn a_mixed_change_raises_deleted_then_created_then_updated_in_order() {
        let before = set(&["https://example.com/keep", "https://example.com/drop"]);
        let after = set(&["https://example.com/keep", "https://example.com/new"]);
        let groups = attribute_change_groups(&before, &after, &[name("https://example.com/new"), name("https://example.com/keep")]);
        pretty_assertions::assert_eq!(groups.len(), 3);
        pretty_assertions::assert_eq!(groups[0].trigger, NotificationTrigger::AttributeDeleted);
        pretty_assertions::assert_eq!(groups[0].attributes, vec![name("https://example.com/drop")]);
        pretty_assertions::assert_eq!(groups[1].trigger, NotificationTrigger::AttributeCreated);
        pretty_assertions::assert_eq!(groups[1].attributes, vec![name("https://example.com/new")]);
        pretty_assertions::assert_eq!(groups[2].trigger, NotificationTrigger::AttributeUpdated);
        pretty_assertions::assert_eq!(groups[2].attributes, vec![name("https://example.com/keep")]);
    }
}
