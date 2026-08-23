//! Turning one entity mutation into the subscriptions it matched.
//!
//! Matching happens in memory against the shared subscription cache, then the
//! entity-level filters (`q`, `geoQ`, `scopeQ`) narrow it further. The records
//! are written in the same transaction as the mutation that raised them, which
//! is what makes a notification crash-consistent with the change it describes.

use crate::{EntityService, NotificationWriteConfig};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_match::SubscriptionMatch};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use andromeda_subscription::{matching::matches_entity_filters, spatial_narrowing::narrow_by_spatial_index};
use chrono::Utc;
use serde_json::Value;
use tracing::debug;

impl EntityService {
    /// Build one record for every subscription this mutation matches.
    ///
    /// Returns an empty vector when notification recording is not configured.
    pub(crate) fn build_subscription_matches(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        trigger: NotificationTrigger,
        changed_attrs: &[AttributeName],
        entity_snapshot: Option<&Value>,
        previous_snapshot: Option<&Value>,
    ) -> Vec<SubscriptionMatch> {
        self.build_subscription_matches_for_instance(
            target,
            entity_id,
            entity_types,
            trigger,
            changed_attrs,
            None,
            entity_snapshot,
            previous_snapshot,
        )
    }

    /// Build durable matches for one precise attribute-instance change.
    pub(crate) fn build_subscription_matches_for_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        trigger: NotificationTrigger,
        changed_attrs: &[AttributeName],
        changed_dataset_id: Option<&Uri>,
        entity_snapshot: Option<&Value>,
        previous_snapshot: Option<&Value>,
    ) -> Vec<SubscriptionMatch> {
        let Some(NotificationWriteConfig { subscription_cache }) = &self.notifications else {
            return Vec::new();
        };

        let event = ChangeEvent::builder()
            .entity_id(entity_id.clone()) // clone: owned Uri for ChangeEvent
            .entity_types(entity_types.to_vec())
            .trigger_reason(trigger)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .changed_attributes(changed_attrs.to_vec())
            .changed_dataset_id(changed_dataset_id.cloned())
            .build();

        let outcome = subscription_cache.find_matching(target, &event);
        debug!(
            entity = %entity_id,
            trigger = %trigger,
            active = outcome.funnel.active,
            routed = outcome.funnel.routed,
            watched = outcome.funnel.watched,
            evaluated = outcome.funnel.evaluated,
            matched = outcome.funnel.matched,
            "Subscription candidate funnel"
        );
        outcome.funnel.record();

        let filter_snapshot = if matches!(trigger, NotificationTrigger::AttributeDeleted | NotificationTrigger::EntityDeleted) {
            previous_snapshot.or(entity_snapshot)
        } else {
            entity_snapshot
        };
        let matching: Vec<_> = if let Some(data) = filter_snapshot {
            narrow_by_spatial_index(subscription_cache, target, outcome.matched, data, entity_types)
                .into_iter()
                .filter(|sub| matches_entity_filters(sub, data))
                .collect()
        } else {
            outcome.matched
        };

        matching
            .into_iter()
            .map(|sub| {
                // clone: every entry owns its own copy of the entity id, the
                // snapshots and the attribute names, all of which are borrowed
                // here.
                SubscriptionMatch::builder()
                    .subscription_key(sub.key)
                    .entity_id(entity_id.clone())
                    .trigger(trigger)
                    .entity_snapshot(entity_snapshot.cloned())
                    .previous_snapshot(previous_snapshot.cloned())
                    .changed_attrs(changed_attrs.to_vec())
                    .changed_dataset_id(changed_dataset_id.cloned())
                    .build()
            })
            .collect()
    }
}
