//! Which candidate subscriptions an entity change satisfies the entity filters
//! of (clause 5.8.6).
//!
//! The subscribed Entities "that changed and that match [...] the query,
//! geoquery and Scope query conditions" are read after the write -- except an
//! Entity the write deleted, which only its previous state describes. A change
//! that removed instances also matches through the state before the write, so a
//! subscription filtering on a removed Attribute still hears of its removal.
//! The durable path that queues a notification in the write's transaction and
//! the live path that sends it at once both decide it here.

use crate::{cache::SubscriptionCache, compiled::CompiledSubscription, matching::matches_entity_filters, spatial_narrowing::narrow_by_spatial_index};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use andromeda_ngsi_ld_query::queried_entity::linked_entities::NoLinkedEntities;
use chrono::Utc;
use std::sync::Arc;

/// The states of the Entity an entity change is filtered against.
#[derive(Clone, Copy, Debug)]
pub struct ChangedStates<'a> {
    /// The Entity as the write left it, absent for a deletion.
    pub current: Option<&'a EntityState>,
    /// The Entity as it stood before the write, absent for a creation.
    pub previous: Option<&'a EntityState>,
}

impl<'a> ChangedStates<'a> {
    /// The states the filters of `change` read: a deletion the state it removed,
    /// any other change the state it left and, when it removed instances, the
    /// state before it.
    #[must_use]
    pub fn filtered(self, change: &EntityChange) -> Vec<&'a EntityState> {
        match change.lifecycle {
            EntityLifecycle::Deleted => self.previous.or(self.current).into_iter().collect(),
            EntityLifecycle::Created | EntityLifecycle::Modified => {
                self.current.into_iter().chain(self.previous.filter(|_| change.deletes_instances())).collect()
            }
        }
    }
}

/// How the `rel{…}` hops of a subscription's filters are decided (clause 4.9).
#[derive(Clone, Copy, Debug)]
pub enum HopResolution<'a> {
    /// The linked entities cannot be read where the change is filtered: the
    /// commit filters inside the write's transaction. A subscription whose
    /// filters hop is admitted undecided, and its delivery decides it.
    Deferred,
    /// The linked entities the hops reach, read from the store.
    Resolved(&'a LinkedStates),
}

/// The candidates whose `q`, `geoQ` and `scopeQ` hold on a state the change is
/// filtered against, in candidate order.
///
/// With no state to read, no filter can be evaluated and the candidates stand.
/// Either state is read as it stands now, without its expired instances
/// (clause 4.22); a state with none expired is read as it is, uncopied. A
/// candidate whose filters hop is decided as `hops` says.
#[must_use]
pub fn narrow_by_change_filters(
    cache: &SubscriptionCache,
    target: &ContextTarget,
    candidates: Vec<Arc<CompiledSubscription>>,
    change: &EntityChange,
    states: ChangedStates<'_>,
    entity_types: &[EntityTypeName],
    hops: HopResolution<'_>,
) -> Vec<Arc<CompiledSubscription>> {
    let filtered = states.filtered(change);
    if filtered.is_empty() {
        return candidates;
    }
    let now = Utc::now();
    let mut matching: Vec<Arc<CompiledSubscription>> = Vec::with_capacity(candidates.len());
    for state in filtered.into_iter().map(|state| state.standing_at(now)) {
        let remaining: Vec<Arc<CompiledSubscription>> = candidates
            .iter()
            .filter(|candidate| !matching.iter().any(|kept| kept.key == candidate.key))
            .map(Arc::clone)
            .collect();
        matching.extend(
            narrow_by_spatial_index(cache, target, remaining, &state, entity_types)
                .into_iter()
                .filter(|subscription| admitted(subscription, &state, hops)),
        );
    }
    matching
}

/// Whether one candidate's entity filters admit `state`.
fn admitted(subscription: &CompiledSubscription, state: &EntityState, hops: HopResolution<'_>) -> bool {
    if !subscription.filters_hop() {
        return matches_entity_filters(subscription, state, NoLinkedEntities);
    }
    match hops {
        HopResolution::Deferred => true,
        HopResolution::Resolved(linked) => matches_entity_filters(subscription, state, linked),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cache::SubscriptionCache,
        change_filters::{ChangedStates, HopResolution, narrow_by_change_filters},
        compiled::CompiledSubscription,
        geo_filter::SubscriptionGeoFilter,
    };
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        queried::linked_states::LinkedStates,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::NotificationTrigger,
        subscription::SubscriptionStatus,
    };
    use andromeda_ngsi_ld_query::{filter::parser::parse, projection::compiled::Projection};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::{num::NonZeroU32, sync::Arc};
    use url::Url;

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const PARKING: &str = "urn:ngsi-ld:OffStreetParking:Downtown1";

    /// A subscription filtering on `q`, keyed `key`.
    fn subscription(key: i64, q: &str) -> Arc<CompiledSubscription> {
        Arc::new(CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{key}").parse().expect("valid URI"),
            key: SubscriptionKey::try_from(key).expect("a valid key"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: vec![NotificationTrigger::EntityCreated],
            watched_attrs: None,
            selectors: Vec::new(),
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: Some(parse(q).expect("the filter parses")),
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: Projection::default(),
        })
    }

    fn state(document: &serde_json::Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes")
    }

    /// A Vehicle parked in the Downtown parking, travelling at 80.
    fn vehicle() -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]}],
            "https://example.com/isParkedIn": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": PARKING}]
            }]
        }]))
    }

    /// The Downtown parking of `category`.
    fn parking(category: &str) -> LinkedStates {
        let mut linked = LinkedStates::new();
        linked.insert(Arc::new(state(&json!([{
            "@id": PARKING,
            "@type": ["https://example.com/OffStreetParking"],
            "https://example.com/category": [{"@type": [PROPERTY], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]}]
        }]))));
        linked
    }

    fn creation() -> EntityChange {
        EntityChange::new(
            EntityLifecycle::Created,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: "https://example.com/speed".parse().expect("valid attribute name"),
                    dataset_id: None,
                },
                kind: AttributeChangeKind::Created,
            }],
        )
    }

    const COMMERCIAL_HOP: &str = "\"https://example.com/isParkedIn\"{\"https://example.com/category\"}==\"commercial\"";

    /// The keys of the candidates `hops` admits for the creation of the
    /// Vehicle.
    fn admitted_keys(candidates: Vec<Arc<CompiledSubscription>>, hops: HopResolution<'_>) -> Vec<String> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let current = vehicle();
        let states = ChangedStates {
            current: Some(&current),
            previous: None,
        };
        narrow_by_change_filters(&cache, &ContextTarget::Live, candidates, &creation(), states, &[], hops)
            .into_iter()
            .map(|subscription| subscription.id.to_string())
            .collect()
    }

    /// The commit cannot read the linked Parking, so the hopping subscription
    /// is admitted for its delivery to decide.
    #[test]
    fn a_deferred_hop_admits_the_subscription_undecided() {
        pretty_assertions::assert_eq!(
            admitted_keys(vec![subscription(1, COMMERCIAL_HOP)], HopResolution::Deferred),
            vec!["urn:ngsi-ld:Subscription:1".to_owned()]
        );
    }

    /// Clause 4.9 EXAMPLE 13: with the Parking read, the hop decides.
    #[test]
    fn a_resolved_hop_decides_the_subscription() {
        let commercial = parking("commercial");
        let residential = parking("residential");
        pretty_assertions::assert_eq!(
            admitted_keys(vec![subscription(1, COMMERCIAL_HOP)], HopResolution::Resolved(&commercial)),
            vec!["urn:ngsi-ld:Subscription:1".to_owned()]
        );
        pretty_assertions::assert_eq!(
            admitted_keys(vec![subscription(1, COMMERCIAL_HOP)], HopResolution::Resolved(&residential)),
            Vec::<String>::new()
        );
    }

    /// A filter without a hop is decided wherever the change is filtered.
    #[test]
    fn a_subscription_without_a_hop_is_decided_either_way() {
        let commercial = parking("commercial");
        for hops in [HopResolution::Deferred, HopResolution::Resolved(&commercial)] {
            pretty_assertions::assert_eq!(
                admitted_keys(
                    vec![
                        subscription(1, "\"https://example.com/speed\">50"),
                        subscription(2, "\"https://example.com/speed\">100")
                    ],
                    hops
                ),
                vec!["urn:ngsi-ld:Subscription:1".to_owned()]
            );
        }
    }
}
