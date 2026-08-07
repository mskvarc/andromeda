//! Turning one entity mutation into the outbox entries its subscribers need.
//!
//! Matching happens in memory against the shared subscription cache, then the
//! entity-level filters (`q`, `geoQ`, `scopeQ`) narrow it further. The entries
//! are written in the same transaction as the mutation that raised them, which
//! is what makes a notification crash-consistent with the change it describes.

use crate::{
    event::change_event::{ChangeEvent, ChangeOrigin},
    service::{
        entity::{EntityService, OutboxWriteConfig},
        outbox::entry::NewOutboxEntry,
        subscription_matching::matches_entity_filters,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use chrono::Utc;
use serde_json::Value;

impl EntityService {
    /// Build outbox entries for every subscription this mutation matches.
    ///
    /// Returns an empty vector when the outbox is not configured.
    pub(crate) fn build_outbox_entries(
        &self,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        trigger: NotificationTrigger,
        changed_attrs: &[AttributeName],
        entity_snapshot: Option<&Value>,
        previous_snapshot: Option<&Value>,
    ) -> Vec<NewOutboxEntry> {
        let Some(OutboxWriteConfig { subscription_cache }) = &self.outbox else {
            return Vec::new();
        };

        let event = ChangeEvent::builder()
            .entity_id(entity_id.clone()) // clone: owned Uri for ChangeEvent
            .entity_types(entity_types.to_vec())
            .trigger_reason(trigger)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .changed_attributes(changed_attrs.to_vec())
            .build();

        let matching = subscription_cache.find_matching(&event);

        let matching: Vec<_> = if let Some(data) = entity_snapshot {
            matching.into_iter().filter(|sub| matches_entity_filters(sub, data)).collect()
        } else {
            matching
        };

        matching
            .into_iter()
            .map(|sub| {
                // clone: every entry owns its own copy of the subscription and
                // entity ids, the snapshots and the attribute names, all of which
                // are borrowed here.
                NewOutboxEntry::builder()
                    .subscription_id(sub.id.clone())
                    .entity_id(entity_id.clone())
                    .trigger(trigger)
                    .entity_snapshot(entity_snapshot.cloned())
                    .previous_snapshot(previous_snapshot.cloned())
                    .changed_attrs(changed_attrs.to_vec())
                    .build()
            })
            .collect()
    }
}
