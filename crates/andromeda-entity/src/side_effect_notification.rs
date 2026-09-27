//! Settling the notifications a drained side-effect job still owes.
//!
//! A broker that records durable deliveries at commit queues a write's
//! notifications in the write's own transaction, so the side-effect job it
//! defers carries no change for them. A live-delivery broker records none at
//! commit: its job carries the changes, and whichever broker drains the job owes
//! their notifications. Brokers sharing a tenant database may run different
//! delivery modes, so the draining broker settles them by its own mode, not the
//! producer's: a live-delivery broker hands them to its in-process notification
//! worker, and a durable-delivery broker -- whose worker never evaluates an
//! in-process change -- matches them against its subscription cache and queues
//! the deliveries in the job's replay commit. Either way each change is notified
//! once.

use crate::{
    NotificationWriteConfig,
    change_sink::EntityChangeSink,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    sys_attrs::snapshot_with_sys_attrs,
};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use std::sync::Arc;

/// How the broker draining a side-effect job settles the notifications the job
/// still owes, following the broker's own delivery mode.
pub enum SideEffectNotificationRoute {
    /// Publish the owed changes to this broker's in-process notification
    /// worker once the replay commit that settled the job has landed.
    Live(Arc<dyn EntityChangeSink>),
    /// Queue a durable delivery for every subscription an owed change matches,
    /// in the job's replay commit.
    Durable(NotificationWriteConfig),
}

/// The durable matches every owed change in `changes` is due, in the order the
/// changes were raised.
///
/// The order is what numbers the notification events the replay commit writes,
/// following the ordinals the queuing commit already used, so the events keep
/// the order their changes were raised in.
pub(crate) fn owed_subscription_matches(recording: &NotificationWriteConfig, target: &ContextTarget, changes: &[ChangeEvent]) -> Vec<SubscriptionMatch> {
    changes.iter().flat_map(|change| change_matches(recording, target, change)).collect()
}

/// The durable matches one owed change is due, built exactly as the committing
/// broker would have built them had it recorded durable deliveries itself.
///
/// A creation or update carries the entity the write left behind, which is both
/// what is filtered and what is notified. A deletion carries no resulting
/// entity, only what was removed: the match stores that with its clause 4.8
/// system attributes embedded, because the row is gone by delivery time and
/// `createdAt` and `modifiedAt` can no longer be read back (clause 5.8.6).
fn change_matches(recording: &NotificationWriteConfig, target: &ContextTarget, change: &ChangeEvent) -> Vec<SubscriptionMatch> {
    let removed = match (change.current_payload.as_deref(), change.entity_snapshot.as_deref()) {
        (Some(_), _) | (None, None) => None,
        (None, Some(snapshot)) => Some(match change.sys_attrs_snapshot.as_ref() {
            Some(sys_attrs) => snapshot_with_sys_attrs(snapshot, sys_attrs),
            None => snapshot.clone(), // clone: the durable match owns the removed entity while the change keeps its shared snapshot.
        }),
    };
    recording.subscription_matches(
        ChangedEntity {
            target,
            id: &change.entity_id,
            types: &change.entity_types,
        },
        change.trigger_reason,
        &change.changed_attributes,
        change.changed_dataset_id.as_ref(),
        MutationSnapshots {
            current: change.current_payload.as_deref().or(removed.as_ref()),
            previous: change.previous_attributes.as_deref(),
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        NotificationWriteConfig,
        building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache, subscription_key},
        side_effect_notification::owed_subscription_matches,
    };
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
    use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger, SysAttrsSnapshot};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const ENTITY_ID: &str = "urn:ngsi-ld:Building:owed";
    const CATEGORY_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/category";

    async fn recording() -> NotificationWriteConfig {
        NotificationWriteConfig {
            subscription_cache: building_subscription_cache().await,
        }
    }

    fn building(category: &str) -> Value {
        json!([{
            "@id": ENTITY_ID,
            "@type": [BUILDING_TYPE_IRI],
            CATEGORY_IRI: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]
            }]
        }])
    }

    fn change(trigger: NotificationTrigger, types: &[&str]) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(ENTITY_ID.parse::<Uri>().expect("a valid entity URI"))
            .entity_types(types.iter().map(|raw| raw.parse::<EntityTypeName>().expect("a valid type name")).collect())
            .trigger_reason(trigger)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .changed_attributes(vec![CATEGORY_IRI.parse::<AttributeName>().expect("a valid attribute name")])
            .build()
    }

    /// Clause 5.8.6: an update owes the subscription the entity the write left
    /// behind, with the state it replaced for `showChanges`.
    #[tokio::test]
    async fn an_owed_update_matches_the_subscription_with_the_resulting_entity() {
        let mut updated = change(NotificationTrigger::AttributeUpdated, &[BUILDING_TYPE_IRI]);
        updated.current_payload = Some(Arc::new(building("retail")));
        updated.previous_attributes = Some(Arc::new(building("office")));

        let matches = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[updated]);

        assert_eq!(matches.len(), 1, "the one Building subscription is owed the update");
        assert_eq!(matches[0].subscription_key, subscription_key());
        assert_eq!(matches[0].trigger, NotificationTrigger::AttributeUpdated);
        assert_eq!(matches[0].entity_snapshot, Some(building("retail")));
        assert_eq!(matches[0].previous_snapshot, Some(building("office")));
    }

    /// A change to an entity no subscription selects is owed nothing.
    #[tokio::test]
    async fn an_owed_change_no_subscription_selects_is_owed_nothing() {
        let mut created = change(NotificationTrigger::EntityCreated, &["https://example.org/Vehicle"]);
        created.current_payload = Some(Arc::new(building("office")));

        assert!(owed_subscription_matches(&recording().await, &ContextTarget::Live, &[created]).is_empty());
    }

    /// A deletion carries only what was removed; the match stores it with the
    /// clause 4.8 system attributes embedded, as the committing broker would.
    #[tokio::test]
    async fn an_owed_deletion_matches_with_the_removed_entity_and_its_system_attributes() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant");
        let mut deleted = change(NotificationTrigger::EntityDeleted, &[BUILDING_TYPE_IRI]);
        deleted.entity_snapshot = Some(Arc::new(building("office")));
        deleted.previous_attributes = Some(Arc::new(building("office")));
        deleted.sys_attrs_snapshot = Some(SysAttrsSnapshot {
            created_at: instant,
            modified_at: instant,
            expires_at: None,
        });

        let matches = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[deleted]);

        assert_eq!(matches.len(), 1);
        let stored = matches[0].entity_snapshot.as_ref().expect("the deletion stores the removed entity");
        assert!(stored[0].get(expanded_properties::CREATED_AT.as_str()).is_some(), "{stored}");
        assert_eq!(matches[0].previous_snapshot, Some(building("office")));
    }

    /// The order the changes were raised in numbers the replay commit's
    /// notification events, so the matches keep it.
    #[tokio::test]
    async fn owed_changes_keep_the_order_they_were_raised_in() {
        let mut first = change(NotificationTrigger::EntityCreated, &[BUILDING_TYPE_IRI]);
        first.current_payload = Some(Arc::new(building("office")));
        let mut second = change(NotificationTrigger::AttributeCreated, &[BUILDING_TYPE_IRI]);
        second.current_payload = Some(Arc::new(building("office")));

        let triggers: Vec<NotificationTrigger> = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[first, second])
            .iter()
            .map(|matched| matched.trigger)
            .collect();

        assert_eq!(triggers, vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated]);
    }
}
