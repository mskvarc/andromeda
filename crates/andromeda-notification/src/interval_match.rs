//! Matching entity states against a periodic subscription's filters (clause
//! 5.8.6).
//!
//! A periodic notification reads entities a page at a time and decides in memory
//! which ones the subscription's `q`, `geoQ`, `scopeQ` and `csf` accept, through
//! the same entity filters a change-driven notification applies, because a
//! boolean selector cannot be pushed into a `WHERE`.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, query::EntityQueryService};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_query::filter::expression::Expr;
use andromeda_subscription::{compiled::CompiledSubscription, matching::matches_entity_filters};
use chrono::{DateTime, Utc};

/// The entities of a page the subscription's filters accept as they stand at
/// `at`, the entities a `rel{…}` hop of its `q` or `csf` crosses into read
/// from the store (clause 4.9).
///
/// # Errors
///
/// Returns the store's [`EntityError`] when the linked entities cannot be read.
pub(crate) async fn matching_entities(
    entity_query: &EntityQueryService,
    target: &ContextTarget,
    sub: &CompiledSubscription,
    entities: Vec<EntityState>,
    at: DateTime<Utc>,
) -> Result<Vec<EntityState>, EntityError> {
    let entities = standing_at(entities, at);
    let hops: Vec<&Expr> = sub.hopping_filters().collect();
    let linked = if hops.is_empty() {
        LinkedStates::new()
    } else {
        let roots: Vec<&EntityState> = entities.iter().collect();
        entity_query.linked_filter_states(target, &roots, &hops, at).await?
    };
    Ok(admitted(sub, entities, &linked))
}

/// The entities as they stand at `at`.
///
/// Clause 4.22: an attribute instance past its `expiresAt` is invalid, so it
/// is removed before any filter reads the entity, and the entities returned,
/// the ones the notification carries, no longer hold it either.
fn standing_at(mut entities: Vec<EntityState>, at: DateTime<Utc>) -> Vec<EntityState> {
    for entity in &mut entities {
        entity.remove_expired_instances(at);
    }
    entities
}

/// The entities the subscription's entity filters admit, a hop crossing into
/// `linked`.
fn admitted(sub: &CompiledSubscription, entities: Vec<EntityState>, linked: &LinkedStates) -> Vec<EntityState> {
    entities.into_iter().filter(|entity| matches_entity_filters(sub, entity, linked)).collect()
}

#[cfg(test)]
mod tests {
    use crate::interval_match::{admitted, standing_at};
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        queried::linked_states::LinkedStates,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        subscription::Subscription,
    };
    use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
    use andromeda_subscription::{
        cache::SubscriptionCache,
        change_filters::{ChangedStates, HopResolution, narrow_by_change_filters},
        compilation::compile_subscription,
        compiled::CompiledSubscription,
        notifying_since::NotifyingSince,
        stored_subscription::StoredSubscription,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const SPEED: &str = "https://example.org/speed";

    /// A periodic subscription on `q=speed>10`, its `q` already expanded.
    fn speeding() -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:speeding",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Vehicle"}],
            "q": format!("\"{SPEED}\">10"),
            "timeInterval": 60,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    /// The entities `subscription` notifies of, as they stand at `at`, a hop
    /// crossing into `linked`.
    fn filtered(subscription: Subscription, entities: Vec<EntityState>, at: DateTime<Utc>, linked: &LinkedStates) -> Vec<EntityState> {
        admitted(&compiled(subscription), standing_at(entities, at), linked)
    }

    fn compiled(subscription: Subscription) -> CompiledSubscription {
        compile_subscription(
            &StoredSubscription {
                key: SubscriptionKey::try_from(1).expect("a valid key"),
                subscription,
                notifying_since: NotifyingSince {
                    active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                    selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                },
            },
            LanguageMatching::TagEquality,
        )
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("instant parses").get()
    }

    /// A Vehicle whose `speed` is 20 until `expires_at`.
    fn vehicle(expires_at: &str) -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            SPEED: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": expires_at}]
            }]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    /// Clause 4.22: an instance past its `expiresAt` is invalid, so a periodic
    /// `q` cannot match through it while the sweep has yet to delete it.
    #[test]
    fn a_periodic_q_does_not_match_through_an_expired_instance() {
        let matched = filtered(
            speeding(),
            vec![vehicle("2030-01-01T00:00:00Z")],
            instant("2030-01-01T00:00:00Z"),
            &LinkedStates::new(),
        );
        assert_eq!(matched, Vec::<EntityState>::new());
    }

    #[test]
    fn a_periodic_q_matches_through_a_standing_instance() {
        let matched = filtered(
            speeding(),
            vec![vehicle("2030-01-01T00:00:00Z")],
            instant("2029-01-01T00:00:00Z"),
            &LinkedStates::new(),
        );
        assert_eq!(matched, vec![vehicle("2030-01-01T00:00:00Z")]);
    }

    /// Clause 5.8.6: "If none of query, geoquery and Scope query are defined,
    /// then all subscribed Entities shall be included". A stored `geoQ` that no
    /// longer compiles is defined, so the periodic notification carries no
    /// entity rather than every one.
    #[test]
    fn a_periodic_geo_q_that_no_longer_compiles_carries_no_entity() {
        let mut subscription = serde_json::to_value(speeding()).expect("the subscription serialises");
        subscription["q"] = Value::Null;
        // Coordinates a string encodes are read when the `geoQ` is compiled, so
        // ones that are no JSON make a stored `geoQ` that no longer compiles.
        subscription["geoQ"] = json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": "[1, "});
        let subscription: Subscription = serde_json::from_value(subscription).expect("valid subscription");
        let matched = filtered(
            subscription,
            vec![vehicle("2030-01-01T00:00:00Z")],
            instant("2029-01-01T00:00:00Z"),
            &LinkedStates::new(),
        );
        assert_eq!(matched, Vec::<EntityState>::new());
    }

    /// Clause 4.9 EXAMPLE 13: a periodic `q` hop reads the entity the
    /// Relationship names, as the sweep gathered it.
    #[test]
    fn a_periodic_hop_reads_the_linked_entity() {
        let subscription: Subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:parked",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Vehicle"}],
            "q": "\"https://example.org/isParkedIn\"{\"https://example.org/category\"}==\"commercial\"",
            "timeInterval": 60,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        let parked = |vehicle: &str, parking: &str| {
            let document = json!([{
                "@id": vehicle,
                "@type": ["https://example.org/Vehicle"],
                "https://example.org/isParkedIn": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                    "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": parking}]
                }]
            }]);
            EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
        };
        let mut linked = LinkedStates::new();
        for (parking, category) in [("urn:p1", "commercial"), ("urn:p2", "residential")] {
            let document = json!([{
                "@id": parking,
                "@type": ["https://example.org/Parking"],
                "https://example.org/category": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": category}]
                }]
            }]);
            linked.insert(Arc::new(
                EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes"),
            ));
        }
        let matched = filtered(
            subscription,
            vec![parked("urn:v1", "urn:p1"), parked("urn:v2", "urn:p2")],
            instant("2029-01-01T00:00:00Z"),
            &linked,
        );
        assert_eq!(matched, vec![parked("urn:v1", "urn:p1")]);
    }

    /// Clause 4.9: on an entity subscription `csf` is a second Query Language
    /// expression over the notified entity, so a periodic notification applies
    /// it as a change-driven one does.
    #[test]
    fn a_periodic_subscription_applies_its_csf() {
        let with_csf = |csf: &str| {
            let mut subscription = serde_json::to_value(speeding()).expect("the subscription serialises");
            subscription["q"] = Value::Null;
            subscription["csf"] = json!(csf);
            serde_json::from_value::<Subscription>(subscription).expect("valid subscription")
        };
        let at = instant("2029-01-01T00:00:00Z");

        let refused = filtered(
            with_csf(&format!("\"{SPEED}\">30")),
            vec![vehicle("2030-01-01T00:00:00Z")],
            at,
            &LinkedStates::new(),
        );
        let kept = filtered(
            with_csf(&format!("\"{SPEED}\">10")),
            vec![vehicle("2030-01-01T00:00:00Z")],
            at,
            &LinkedStates::new(),
        );

        assert_eq!(refused, Vec::<EntityState>::new());
        assert_eq!(kept, vec![vehicle("2030-01-01T00:00:00Z")]);
    }

    /// A Vehicle with a `speed` of 20 until `expires_at`, a `location` in
    /// Heidelberg and the scope `/Spain`.
    fn located_vehicle(expires_at: &str) -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            SPEED: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": expires_at}]
            }],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 8.684_783}, {"@value": 49.406_132}]}]
                }]
            }],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Spain"}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    /// A periodic subscription whose only entity filter is `member`.
    fn filtering_on(member: &str, filter: &Value) -> Subscription {
        let mut subscription = serde_json::to_value(speeding()).expect("the subscription serialises");
        subscription["q"] = Value::Null;
        subscription[member] = filter.clone();
        serde_json::from_value(subscription).expect("valid subscription")
    }

    /// Whether the periodic notification at `at` carries the entity.
    fn interval_carries(subscription: Subscription, entity: EntityState, at: DateTime<Utc>) -> bool {
        !filtered(subscription, vec![entity], at, &LinkedStates::new()).is_empty()
    }

    /// Whether the creation of the entity notifies the subscription. A
    /// Snapshot branch has no spatial index, so the `geoQ` predicate decides.
    fn change_notifies(subscription: Subscription, entity: &EntityState) -> bool {
        let target = ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(3).expect("positive key"),
            "urn:ngsi-ld:Snapshot:parity".parse().expect("valid snapshot uri"),
        ));
        let creation = EntityChange::new(
            EntityLifecycle::Created,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: SPEED.parse().expect("valid attribute name"),
                    dataset_id: None,
                },
                kind: AttributeChangeKind::Created,
            }],
        );
        let states = ChangedStates {
            current: Some(entity),
            previous: None,
        };
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        !narrow_by_change_filters(
            &cache,
            &target,
            vec![Arc::new(compiled(subscription))],
            &creation,
            states,
            &[],
            HopResolution::Deferred,
        )
        .is_empty()
    }

    /// Clause 5.8.6: a periodic notification and a change-driven one decide
    /// `q`, `geoQ`, `scopeQ` and `csf` alike, so one entity is kept or
    /// refused by both.
    #[test]
    fn an_interval_and_a_change_keep_and_refuse_the_same_entity_by_each_filter() {
        let at = instant("2029-01-01T00:00:00Z");
        let live = "2099-01-01T00:00:00Z";
        let near = json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"});
        let far = json!({"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"});
        let cases = [
            ("q", json!(format!("\"{SPEED}\">10")), json!(format!("\"{SPEED}\">30"))),
            ("csf", json!(format!("\"{SPEED}\">10")), json!(format!("\"{SPEED}\">30"))),
            ("scopeQ", json!("/Spain"), json!("/France")),
            ("geoQ", near, far),
        ];
        for (member, kept, refused) in cases {
            assert_eq!(
                (
                    member,
                    interval_carries(filtering_on(member, &kept), located_vehicle(live), at),
                    change_notifies(filtering_on(member, &kept), &located_vehicle(live)),
                ),
                (member, true, true),
            );
            assert_eq!(
                (
                    member,
                    interval_carries(filtering_on(member, &refused), located_vehicle(live), at),
                    change_notifies(filtering_on(member, &refused), &located_vehicle(live)),
                ),
                (member, false, false),
            );
        }
    }

    /// Clause 4.22: an instance past its `expiresAt` is invalid on both paths,
    /// so a filter reading it refuses the entity on both.
    #[test]
    fn an_interval_and_a_change_refuse_an_entity_whose_instance_has_expired() {
        let filter = || filtering_on("csf", &json!(format!("\"{SPEED}\">10")));
        let expired = "2020-01-01T00:00:00Z";
        assert_eq!(
            (
                interval_carries(filter(), located_vehicle(expired), instant("2029-01-01T00:00:00Z")),
                change_notifies(filter(), &located_vehicle(expired)),
            ),
            (false, false)
        );
    }
}
