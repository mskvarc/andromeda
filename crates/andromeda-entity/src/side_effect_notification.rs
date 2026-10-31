//! Settling the notifications a drained side-effect job still owes.
//!
//! A broker that records durable deliveries at commit queues a write's
//! notifications in the write's own transaction, so the side-effect job it
//! defers carries no change for them. A live-delivery broker records none at
//! commit: its job carries the changes, and whichever broker drains the job owes
//! their notifications. Brokers sharing a tenant database may run different
//! delivery modes, so the draining broker settles them by its own mode, not the
//! producer's: a live-delivery broker hands them to its in-process notification
//! worker, and a durable-delivery broker, whose worker never evaluates an
//! in-process change, matches them against its subscription cache and queues
//! the deliveries in the job's replay commit. Either way each change is notified
//! once.

use crate::{
    NotificationWriteConfig,
    change_sink::EntityChangeSink,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    snapshot_supply::SnapshotSupply,
};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
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
/// entity, only what was removed. Either way the match stores the clause 4.8
/// system attributes the change recorded beside the state: the notification
/// reports the change as it left the Entity (clause 5.8.6), and a deleted row
/// cannot be read back at all.
fn change_matches(recording: &NotificationWriteConfig, target: &ContextTarget, change: &ChangeEvent) -> Vec<SubscriptionMatch> {
    let sys_attrs = change.sys_attrs_snapshot.as_ref();
    let current = match (change.current_payload.as_ref(), change.change.lifecycle, change.entity_snapshot.as_ref()) {
        (Some(state), _, _) | (None, EntityLifecycle::Deleted, Some(state)) => SnapshotSupply::Carried { state, sys_attrs },
        (None, EntityLifecycle::Created | EntityLifecycle::Modified | EntityLifecycle::Deleted, _) => SnapshotSupply::Absent,
    };
    recording.subscription_matches(
        ChangedEntity {
            target,
            id: &change.entity_id,
            types: &change.entity_types,
        },
        &change.change,
        MutationSnapshots {
            current,
            previous: change.previous_attributes.as_ref(),
        },
    )
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        NotificationWriteConfig,
        building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache, subscription_key},
        side_effect_notification::owed_subscription_matches,
    };
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::{ChangeOrigin, SysAttrsSnapshot},
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::Arc;

    const ENTITY_ID: &str = "urn:ngsi-ld:Building:owed";
    const CATEGORY_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/category";

    async fn recording() -> NotificationWriteConfig {
        NotificationWriteConfig {
            subscription_cache: building_subscription_cache().await,
        }
    }

    fn building(category: &str) -> EntityState {
        let stored = json!([{
            "@id": ENTITY_ID,
            "@type": [BUILDING_TYPE_IRI],
            CATEGORY_IRI: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]
            }]
        }]);
        EntityState::from_slice(stored.to_string().as_bytes()).expect("the fixture is a stored entity")
    }

    /// A change through `lifecycle` that `kind`s the `category` instance.
    fn change(lifecycle: EntityLifecycle, kind: AttributeChangeKind, types: &[&str]) -> ChangeEvent {
        let category = AttributeChange {
            key: AttributeInstanceKey {
                attr: CATEGORY_IRI.parse::<AttributeName>().expect("a valid attribute name"),
                dataset_id: None,
            },
            kind,
        };
        ChangeEvent::builder()
            .entity_id(ENTITY_ID.parse::<Identifier>().expect("a valid entity URI"))
            .entity_types(types.iter().map(|raw| raw.parse::<EntityTypeName>().expect("a valid type name")).collect())
            .change(EntityChange::new(lifecycle, vec![category]))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    /// Clause 5.8.6: an update owes the subscription the entity the write left
    /// behind, with the state it replaced for `showChanges` and the clause 4.8
    /// system attributes the write left it with.
    #[tokio::test]
    async fn an_owed_update_matches_the_subscription_with_the_resulting_entity() {
        let sys_attrs = SysAttrsSnapshot {
            created_at: Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant"),
            modified_at: Utc.timestamp_opt(1_700_000_100, 0).single().expect("a valid instant"),
            expires_at: None,
        };
        let mut updated = change(EntityLifecycle::Modified, AttributeChangeKind::Updated, &[BUILDING_TYPE_IRI]);
        updated.current_payload = Some(Arc::new(building("retail")));
        updated.previous_attributes = Some(Arc::new(building("office")));
        updated.sys_attrs_snapshot = Some(sys_attrs.clone()); // clone: compared with what the match stores

        let matches = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[updated]);

        assert_eq!(matches.len(), 1, "the one Building subscription is owed the update");
        assert_eq!(matches[0].subscription_key, subscription_key());
        assert_eq!(matches[0].change.lifecycle, EntityLifecycle::Modified);
        assert_eq!(matches[0].entity_snapshot.as_deref(), Some(&building("retail")));
        assert_eq!(matches[0].previous_snapshot.as_deref(), Some(&building("office")));
        assert_eq!(matches[0].sys_attrs_snapshot, Some(sys_attrs));
    }

    /// A change to an entity no subscription selects is owed nothing.
    #[tokio::test]
    async fn an_owed_change_no_subscription_selects_is_owed_nothing() {
        let mut created = change(EntityLifecycle::Created, AttributeChangeKind::Created, &["https://example.org/Vehicle"]);
        created.current_payload = Some(Arc::new(building("office")));

        assert!(owed_subscription_matches(&recording().await, &ContextTarget::Live, &[created]).is_empty());
    }

    /// A deletion carries only what was removed; the match stores it with the
    /// clause 4.8 system attributes beside it, as the committing broker would.
    #[tokio::test]
    async fn an_owed_deletion_matches_with_the_removed_entity_and_its_system_attributes() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant");
        let mut deleted = change(EntityLifecycle::Deleted, AttributeChangeKind::Deleted, &[BUILDING_TYPE_IRI]);
        deleted.entity_snapshot = Some(Arc::new(building("office")));
        deleted.previous_attributes = Some(Arc::new(building("office")));
        let sys_attrs = SysAttrsSnapshot {
            created_at: instant,
            modified_at: instant,
            expires_at: None,
        };
        deleted.sys_attrs_snapshot = Some(sys_attrs.clone()); // clone: compared with what the match stores

        let matches = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[deleted]);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].entity_snapshot.as_deref(), Some(&building("office")));
        assert_eq!(matches[0].sys_attrs_snapshot, Some(sys_attrs));
        assert_eq!(matches[0].previous_snapshot.as_deref(), Some(&building("office")));
    }

    /// The order the changes were raised in numbers the replay commit's
    /// notification events, so the matches keep it.
    #[tokio::test]
    async fn owed_changes_keep_the_order_they_were_raised_in() {
        let mut first = change(EntityLifecycle::Created, AttributeChangeKind::Created, &[BUILDING_TYPE_IRI]);
        first.current_payload = Some(Arc::new(building("office")));
        let mut second = change(EntityLifecycle::Modified, AttributeChangeKind::Updated, &[BUILDING_TYPE_IRI]);
        second.current_payload = Some(Arc::new(building("retail")));
        second.previous_attributes = Some(Arc::new(building("office")));

        let lifecycles: Vec<EntityLifecycle> = owed_subscription_matches(&recording().await, &ContextTarget::Live, &[first, second])
            .iter()
            .map(|matched| matched.change.lifecycle)
            .collect();

        assert_eq!(lifecycles, vec![EntityLifecycle::Created, EntityLifecycle::Modified]);
    }
}
