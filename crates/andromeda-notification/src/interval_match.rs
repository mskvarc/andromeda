//! Matching an expanded entity payload against a periodic subscription's
//! selectors and filters (clause 5.8.6).
//!
//! A periodic notification reads entities a page at a time and decides in memory
//! which ones the subscription's `q`, `geoQ` and `scopeQ` accept, because a
//! boolean selector cannot be pushed into a `WHERE`. These are the payload
//! readers and the filter the interval sweep applies.

use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    expanded::{
        attribute_expiry::remove_expired_instances,
        query_match::{matches_geo_query, matches_scope_query},
    },
    subscription::Subscription,
};
use andromeda_ngsi_ld_query::filter::eval;
use andromeda_subscription::compilation::compile_geo_q;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The node of an expanded entity payload, which the store hands back either
/// bare or wrapped in a one-element array.
fn entity_node(entity: &Value) -> Option<&Value> {
    match entity {
        Value::Array(arr) => arr.first(),
        object @ Value::Object(_) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// The expanded type IRIs an entity payload declares.
pub(crate) fn entity_types(entity: &Value) -> Vec<EntityTypeName> {
    entity_node(entity)
        .and_then(|node| node.get("@type"))
        .and_then(Value::as_array)
        .map(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|raw| raw.parse::<EntityTypeName>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The validated `@id` an expanded entity payload carries.
pub(crate) fn entity_id_of(entity: &Value) -> Option<Identifier> {
    entity_node(entity)?.get("@id")?.as_str()?.parse().ok()
}

/// Apply q, geoQ, and scopeQ filters from a subscription to a list of expanded
/// entities, as they stand at `at`.
///
/// Clause 4.22: an attribute instance past its `expiresAt` is invalid, so it
/// is removed before any filter reads the entity, and the entities returned --
/// the ones the notification carries -- no longer hold it either.
pub(crate) fn apply_subscription_filters(sub: &Subscription, mut entities: Vec<Value>, at: DateTime<Utc>) -> Vec<Value> {
    for entity in &mut entities {
        remove_expired_instances(entity, at);
    }
    let compiled_q = sub.filters.q.as_ref();

    let compiled_geo = sub.filters.geo_q.as_ref().and_then(|v| compile_geo_q(v, sub.id.as_ref()));
    let compiled_scope = sub.filters.scope_q.as_ref();

    let has_filters = compiled_q.is_some() || compiled_geo.is_some() || compiled_scope.is_some();
    if !has_filters {
        return entities;
    }

    entities
        .into_iter()
        .filter(|entity| {
            if let Some(expr) = compiled_q
                && !eval::evaluate(expr, entity)
            {
                return false;
            }
            if let Some(ref geo_q) = compiled_geo
                && !matches_geo_query(geo_q, entity)
            {
                return false;
            }
            if let Some(scope_q) = compiled_scope
                && !matches_scope_query(scope_q, entity)
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
    use andromeda_ngsi_ld_model::subscription::Subscription;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

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
    fn vehicle(expires_at: &str) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            SPEED: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": expires_at}]
            }]
        }])
    }

    /// Clause 4.22: an instance past its `expiresAt` is invalid, so a periodic
    /// `q` cannot match through it while the sweep has yet to delete it.
    #[test]
    fn a_periodic_q_does_not_match_through_an_expired_instance() {
        let matched = apply_subscription_filters(&speeding(), vec![vehicle("2030-01-01T00:00:00Z")], instant("2030-01-01T00:00:00Z"));
        assert_eq!(matched, Vec::<Value>::new());
    }

    #[test]
    fn a_periodic_q_matches_through_a_standing_instance() {
        let matched = apply_subscription_filters(&speeding(), vec![vehicle("2030-01-01T00:00:00Z")], instant("2029-01-01T00:00:00Z"));
        assert_eq!(matched, vec![vehicle("2030-01-01T00:00:00Z")]);
    }
}
