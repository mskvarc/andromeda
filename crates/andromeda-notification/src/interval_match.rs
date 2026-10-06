//! Matching entity states against a periodic subscription's filters (clause
//! 5.8.6).
//!
//! A periodic notification reads entities a page at a time and decides in memory
//! which ones the subscription's `q`, `geoQ` and `scopeQ` accept, because a
//! boolean selector cannot be pushed into a `WHERE`. This is the filter the
//! interval sweep applies.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, query::EntityQueryService};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_model::subscription::Subscription;
use andromeda_ngsi_ld_query::filter::eval::evaluate;
use andromeda_subscription::geo_filter::SubscriptionGeoFilter;
use chrono::{DateTime, Utc};

/// The entities of a page the subscription's filters accept as they stand at
/// `at`, the entities a `rel{…}` hop of its `q` crosses into read from the
/// store (clause 4.9).
///
/// # Errors
///
/// Returns the store's [`EntityError`] when the linked entities cannot be read.
pub(crate) async fn matching_entities(
    entity_query: &EntityQueryService,
    target: &ContextTarget,
    sub: &Subscription,
    entities: Vec<EntityState>,
    at: DateTime<Utc>,
) -> Result<Vec<EntityState>, EntityError> {
    let linked = match sub.filters.q.as_ref().filter(|q| q.contains_linked_hop()) {
        Some(q) => {
            let roots: Vec<&EntityState> = entities.iter().collect();
            entity_query.linked_filter_states(target, &roots, &[q], at).await?
        }
        None => LinkedStates::new(),
    };
    Ok(apply_subscription_filters(sub, entities, at, &linked))
}

/// Apply q, geoQ, and scopeQ filters from a subscription to a list of entity
/// states, as they stand at `at`, a hop of `q` crossing into `linked`.
///
/// Clause 4.22: an attribute instance past its `expiresAt` is invalid, so it
/// is removed before any filter reads the entity, and the entities returned --
/// the ones the notification carries -- no longer hold it either.
fn apply_subscription_filters(sub: &Subscription, mut entities: Vec<EntityState>, at: DateTime<Utc>, linked: &LinkedStates) -> Vec<EntityState> {
    for entity in &mut entities {
        entity.remove_expired_instances(at);
    }
    let compiled_q = sub.filters.q.as_ref();

    let compiled_geo = SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref());
    let compiled_scope = sub.filters.scope_q.as_ref();

    let geo_restricts = !matches!(compiled_geo, SubscriptionGeoFilter::Unrestricted);
    let has_filters = compiled_q.is_some() || geo_restricts || compiled_scope.is_some();
    if !has_filters {
        return entities;
    }

    entities
        .into_iter()
        .filter(|entity| {
            if let Some(expr) = compiled_q
                && !evaluate(expr, entity, linked)
            {
                return false;
            }
            if !compiled_geo.admits(|geo_q| geo_q.matches_entity(entity)) {
                return false;
            }
            if let Some(scope_q) = compiled_scope
                && !scope_q.matches_entity(entity)
            {
                return false;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::interval_match::apply_subscription_filters;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, queried::linked_states::LinkedStates};
    use andromeda_ngsi_ld_model::subscription::Subscription;
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

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
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
        let matched = apply_subscription_filters(
            &speeding(),
            vec![vehicle("2030-01-01T00:00:00Z")],
            instant("2030-01-01T00:00:00Z"),
            &LinkedStates::new(),
        );
        assert_eq!(matched, Vec::<EntityState>::new());
    }

    #[test]
    fn a_periodic_q_matches_through_a_standing_instance() {
        let matched = apply_subscription_filters(
            &speeding(),
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
        let matched = apply_subscription_filters(
            &subscription,
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
        let matched = apply_subscription_filters(
            &subscription,
            vec![parked("urn:v1", "urn:p1"), parked("urn:v2", "urn:p2")],
            instant("2029-01-01T00:00:00Z"),
            &linked,
        );
        assert_eq!(matched, vec![parked("urn:v1", "urn:p1")]);
    }
}
