//! Which candidate subscriptions an entity change satisfies the entity filters
//! of (clause 5.8.6).
//!
//! The subscribed Entities "that changed and that match [...] the query,
//! geoquery and Scope query conditions" are read after the write, except an
//! Entity the write deleted, which only its previous state describes. A change
//! that removed instances also matches through the state before the write for a
//! subscription that removal is owed to (`attributeDeleted`, listed or through
//! `entityUpdated`, on a watched Attribute): the removal is the news, so a
//! subscription filtering on the removed Attribute still hears of it. Any other
//! subscription, a Replace Entity's `attributeUpdated` one among them, is read
//! after the write alone, so an Entity the write left outside its filters is
//! not notified. The durable path that queues a notification in the write's
//! transaction and the live path that sends it at once both decide it here.

use crate::{cache::SubscriptionCache, compiled::CompiledSubscription, matching::matches_entity_filters, spatial_narrowing::SpatialReach};
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
    /// The states the filters of `change` read, for any candidate.
    #[must_use]
    pub fn filtered(self, change: &EntityChange) -> Vec<&'a EntityState> {
        self.readings(change).into_iter().map(|(state, _)| state).collect()
    }

    /// The states the filters of `change` read, each with the candidates it is
    /// read for: a deletion the state it removed, any other change the state it
    /// left and, when it removed instances, the state before it for the
    /// candidates that removal is owed to.
    fn readings(self, change: &EntityChange) -> Vec<(&'a EntityState, Readers)> {
        match change.lifecycle {
            EntityLifecycle::Deleted => self.previous.or(self.current).map(|state| (state, Readers::Every)).into_iter().collect(),
            EntityLifecycle::Created | EntityLifecycle::Modified => self
                .current
                .map(|state| (state, Readers::Every))
                .into_iter()
                .chain(
                    self.previous
                        .filter(|_| change.deletes_instances())
                        .map(|state| (state, Readers::OwedTheRemoval)),
                )
                .collect(),
        }
    }
}

/// The candidates one filtered state is read for.
#[derive(Clone, Copy, Debug)]
enum Readers {
    /// Every candidate.
    Every,
    /// Only a candidate the change's removed instances are owed to.
    OwedTheRemoval,
}

impl Readers {
    /// Whether `subscription` reads its filters on the state for `change`.
    fn include(self, subscription: &CompiledSubscription, change: &EntityChange) -> bool {
        match self {
            Readers::Every => true,
            Readers::OwedTheRemoval => change.is_deletion_owed_to(&subscription.triggers, subscription.watched_attrs.as_ref()),
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

/// The candidates whose `q`, `geoQ`, `scopeQ` and `csf` hold on a state the
/// change is filtered against, in candidate order.
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
    let readings = states.readings(change);
    if readings.is_empty() {
        return candidates;
    }
    let now = Utc::now();
    let mut matching: Vec<Arc<CompiledSubscription>> = Vec::with_capacity(candidates.len());
    let mut remaining = candidates;
    for (state, readers) in readings.into_iter().map(|(state, readers)| (state.standing_at(now), readers)) {
        let mut reach = SpatialReach::new(cache, target, &state, entity_types);
        let (admitted_here, rest): (Vec<Arc<CompiledSubscription>>, Vec<Arc<CompiledSubscription>>) = remaining
            .into_iter()
            .partition(|subscription| readers.include(subscription, change) && reach.admits(subscription) && admitted(subscription, &state, hops));
        matching.extend(admitted_here);
        remaining = rest;
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
        notifying_since::NotifyingSince,
    };
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
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
        notification::{DEFAULT_NOTIFICATION_TRIGGERS, NotificationTrigger, effective_notification_triggers},
        subscription::{SubscriptionStatus, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_query::{
        filter::{language_matching::LanguageMatching, parser::parse},
        projection::compiled::Projection,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::{num::NonZeroU32, sync::Arc};
    use url::Url;

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const PARKING: &str = "urn:ngsi-ld:OffStreetParking:Downtown1";

    /// A subscription filtering on `q`, keyed `key`, listening to
    /// `entityCreated`.
    fn subscription(key: i64, q: &str) -> Arc<CompiledSubscription> {
        listening(key, q, vec![NotificationTrigger::EntityCreated])
    }

    /// A subscription filtering on `q`, keyed `key`, listening to `triggers`.
    fn listening(key: i64, q: &str, triggers: Vec<NotificationTrigger>) -> Arc<CompiledSubscription> {
        Arc::new(CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{key}").parse().expect("valid URI"),
            key: SubscriptionKey::try_from(key).expect("a valid key"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers,
            watched_attrs: None,
            selectors: Vec::new(),
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
            compiled_q: Some(parse(q).expect("the filter parses")),
            language_matching: LanguageMatching::TagEquality,
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
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
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

    /// The Vehicle as the write left it, its speed removed.
    fn vehicle_without_speed() -> EntityState {
        state(&json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/isParkedIn": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": PARKING}]
            }]
        }]))
    }

    /// The keys of the candidates admitted for a write that took the Vehicle
    /// from `previous` to `current` through `instances`.
    fn admitted_through(
        candidates: Vec<Arc<CompiledSubscription>>,
        previous: &EntityState,
        current: &EntityState,
        instances: Vec<AttributeChange>,
    ) -> Vec<String> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let states = ChangedStates {
            current: Some(current),
            previous: Some(previous),
        };
        let change = EntityChange::new(EntityLifecycle::Modified, instances);
        narrow_by_change_filters(&cache, &ContextTarget::Live, candidates, &change, states, &[], HopResolution::Deferred)
            .into_iter()
            .map(|subscription| subscription.id.to_string())
            .collect()
    }

    /// The instance of `attribute` going through `kind`.
    fn instance(attribute: &str, kind: AttributeChangeKind) -> AttributeChange {
        AttributeChange {
            key: AttributeInstanceKey {
                attr: attribute.parse().expect("valid attribute name"),
                dataset_id: None,
            },
            kind,
        }
    }

    /// Clause 5.8.6: the candidates the state the write left admits come
    /// first, then those the removal is owed to that only the state before it
    /// admits, each once.
    #[test]
    fn a_removal_lists_what_the_state_left_admits_before_what_only_the_previous_state_admits() {
        let deleted = || vec![NotificationTrigger::AttributeDeleted];
        let candidates = vec![
            listening(1, "\"https://example.com/speed\">50", deleted()),
            listening(2, "\"https://example.com/isParkedIn\"", deleted()),
            listening(3, "\"https://example.com/speed\">100", deleted()),
        ];
        let removal = vec![instance("https://example.com/speed", AttributeChangeKind::Deleted)];
        pretty_assertions::assert_eq!(
            admitted_through(candidates, &vehicle(), &vehicle_without_speed(), removal),
            vec!["urn:ngsi-ld:Subscription:2".to_owned(), "urn:ngsi-ld:Subscription:1".to_owned()]
        );
    }

    /// Clause 5.8.6 matches the Entity that changed: a subscription the
    /// removal is not owed to, under the default triggers of Table 5.2.12-1,
    /// reads the state the write left alone, so a Replace that leaves the
    /// Vehicle outside its `q` notifies it nothing.
    #[test]
    fn a_removal_matches_the_previous_state_only_for_a_subscription_listening_to_the_removal() {
        let defaults = || DEFAULT_NOTIFICATION_TRIGGERS.to_vec();
        let candidates = vec![
            listening(1, "\"https://example.com/speed\">50", defaults()),
            listening(2, "\"https://example.com/isParkedIn\"", defaults()),
            listening(
                3,
                "\"https://example.com/speed\">50",
                effective_notification_triggers(Some(&[NotificationTrigger::EntityUpdated])),
            ),
        ];
        let replace = vec![
            instance("https://example.com/speed", AttributeChangeKind::Deleted),
            instance("https://example.com/isParkedIn", AttributeChangeKind::Updated),
        ];
        pretty_assertions::assert_eq!(
            admitted_through(candidates, &vehicle(), &vehicle_without_speed(), replace),
            vec!["urn:ngsi-ld:Subscription:2".to_owned(), "urn:ngsi-ld:Subscription:3".to_owned()]
        );
    }

    /// Clause 4.9: an attribute path alone holds when the entity defines it,
    /// so a `q` of `speed` admits the Vehicle that has a speed and not the one
    /// without.
    #[test]
    fn an_existence_q_admits_only_an_entity_defining_the_attribute() {
        let defaults = || DEFAULT_NOTIFICATION_TRIGGERS.to_vec();
        let update = || vec![instance("https://example.com/isParkedIn", AttributeChangeKind::Updated)];
        pretty_assertions::assert_eq!(
            admitted_through(
                vec![listening(1, "\"https://example.com/speed\"", defaults())],
                &vehicle(),
                &vehicle(),
                update()
            ),
            vec!["urn:ngsi-ld:Subscription:1".to_owned()]
        );
        pretty_assertions::assert_eq!(
            admitted_through(
                vec![listening(1, "\"https://example.com/speed\"", defaults())],
                &vehicle_without_speed(),
                &vehicle_without_speed(),
                update()
            ),
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
