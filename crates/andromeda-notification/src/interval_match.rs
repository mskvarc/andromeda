//! Matching an expanded entity payload against a periodic subscription's
//! selectors and filters (clause 5.8.6).
//!
//! A periodic notification reads entities a page at a time and decides in memory
//! which ones the subscription's `q`, `geoQ` and `scopeQ` accept, because a
//! boolean selector cannot be pushed into a `WHERE`. These are the payload
//! readers and the filter the interval sweep applies.

use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::query_match::{matches_geo_query, matches_scope_query},
    subscription::Subscription,
};
use andromeda_ngsi_ld_query::filter::eval;
use andromeda_subscription::compilation::compile_geo_q;
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
pub(crate) fn entity_id_of(entity: &Value) -> Option<Uri> {
    entity_node(entity)?.get("@id")?.as_str()?.parse().ok()
}

/// Apply q, geoQ, and scopeQ filters from a subscription to a list of expanded entities.
pub(crate) fn apply_subscription_filters(sub: &Subscription, entities: Vec<Value>) -> Vec<Value> {
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
