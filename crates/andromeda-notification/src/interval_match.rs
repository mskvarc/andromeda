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
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_model::subscription::Subscription;
    use andromeda_subscription::{compilation::compile_subscription, stored_subscription::StoredSubscription};
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
        let compiled = compile_subscription(&StoredSubscription {
            key: SubscriptionKey::try_from(1).expect("a valid key"),
            subscription,
        });
        admitted(&compiled, standing_at(entities, at), linked)
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
}
