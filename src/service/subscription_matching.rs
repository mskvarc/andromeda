//! Subscription matching and entity-level filtering (geoQ, scopeQ, q).
//!
//! Pure domain logic for determining whether an entity change event matches
//! a compiled subscription. Used by the notification worker (event-driven
//! delivery) and the subscription cache (outbox entry generation).

use crate::{
    domain::{
        notification::{ChangeEvent, ChangeOrigin, CompiledSubscription},
        subscription::SubscriptionStatus,
    },
    jsonld::vocab,
    query::q,
};
use chrono::Utc;
use regex::Regex;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// Find matching subscriptions from an arbitrary iterator of compiled subscriptions.
///
/// Filters the given subscriptions against the event using [`matches_subscription`],
/// returning only those that match.
pub fn find_matching(subs: impl Iterator<Item = Arc<CompiledSubscription>>, event: &ChangeEvent) -> Vec<Arc<CompiledSubscription>> {
    subs.filter(|sub| matches_subscription(sub, event)).collect()
}

/// Determine whether a compiled subscription matches a change event.
///
/// Checks status, expiration, trigger, entity selectors, watched attributes,
/// and localOnly filtering (ETSI GS CIM 009 v1.9.1, clause 5.5.13).
pub fn matches_subscription(sub: &CompiledSubscription, event: &ChangeEvent) -> bool {
    if sub.status != SubscriptionStatus::Active {
        debug!("Subscription {} skipped: status={} (expected active)", sub.id, sub.status);
        return false;
    }

    if let Some(expires_at) = sub.expires_at
        && expires_at <= Utc::now()
    {
        debug!("Subscription {} skipped: expired at {}", sub.id, expires_at);
        return false;
    }

    if sub.time_interval_secs.is_some() {
        return false;
    }

    if sub.local_only && event.origin == ChangeOrigin::Forwarded {
        debug!("Subscription {} skipped: localOnly=true but event origin is forwarded", sub.id);
        return false;
    }

    let trigger_str = event.trigger_reason.to_string();
    if !sub.triggers.iter().any(|t| t == &trigger_str) {
        debug!("Subscription {} skipped: trigger '{}' not in {:?}", sub.id, trigger_str, sub.triggers);
        return false;
    }

    if !sub.selectors.is_empty() {
        let entity_id_str = event.entity_id.to_string(); // convert once for string-based comparisons below
        let any_selector_matches = sub.selectors.iter().any(|sel| {
            let type_matches = if let Some(ref pattern) = sel.type_pattern {
                event.entity_types.iter().any(|t| pattern.is_match(t))
            } else {
                event.entity_types.contains(&sel.type_)
            };
            if !type_matches {
                debug!(
                    "Subscription {} selector type mismatch: selector={} event_types={:?}",
                    sub.id, sel.type_, event.entity_types
                );
                return false;
            }

            if let Some(ref id) = sel.id
                && id.as_str() != entity_id_str.as_str()
            {
                debug!("Subscription {} selector id mismatch: selector={} event_id={}", sub.id, id, event.entity_id);
                return false;
            }

            if let Some(ref pattern) = sel.id_pattern
                && !pattern.is_match(&entity_id_str)
            {
                debug!(
                    "Subscription {} selector idPattern mismatch: pattern={} event_id={}",
                    sub.id,
                    pattern.as_str(),
                    event.entity_id
                );
                return false;
            }

            true
        });

        if !any_selector_matches {
            debug!("Subscription {} skipped: no selector matched event {}", sub.id, event.entity_id);
            return false;
        }
    }

    if let Some(ref watched) = sub.watched_attrs
        && !event.changed_attributes.iter().any(|a| watched.contains(a))
    {
        debug!(
            "Subscription {} skipped: watchedAttributes {:?} did not match event attrs {:?}",
            sub.id, watched, event.changed_attributes
        );
        return false;
    }

    debug!(
        "Subscription {} matched event {} (trigger={}, types={:?}, attrs={:?})",
        sub.id, event.entity_id, trigger_str, event.entity_types, event.changed_attributes
    );
    true
}

/// Evaluate q, geoQ, scopeQ, and csf filters against the fetched entity data.
///
/// Returns `true` if the entity passes all configured filters on the subscription.
/// Filters that are not configured (None) are skipped.
pub fn matches_entity_filters(sub: &CompiledSubscription, entity_data: &Value) -> bool {
    if let Some(ref expr) = sub.compiled_q
        && !q::eval::evaluate(expr, entity_data)
    {
        debug!("Subscription {} skipped: q filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref geo_q) = sub.geo_q
        && !matches_geo_filter(geo_q, entity_data)
    {
        debug!("Subscription {} skipped: geoQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref scope_q) = sub.scope_q
        && !matches_scope_filter(scope_q, entity_data)
    {
        debug!("Subscription {} skipped: scopeQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref csf_expr) = sub.compiled_csf
        && !q::eval::evaluate(csf_expr, entity_data)
    {
        debug!("Subscription {} skipped: csf filter did not match entity", sub.id);
        return false;
    }

    true
}

/// Evaluate a geo-spatial filter (geoQ) against entity data.
///
/// Supports georel values: within, contains, intersects, disjoint, equals,
/// overlaps, and near (with maxDistance/minDistance modifiers).
pub fn matches_geo_filter(geo_q: &Value, entity_data: &Value) -> bool {
    use geo::algorithm::{Contains, Intersects};

    let geo_obj = match geo_q.as_object() {
        Some(o) => o,
        None => return true,
    };

    let georel_str = match geo_obj.get("georel").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return true,
    };
    let geometry_str = match geo_obj.get("geometry").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return true,
    };
    let coordinates_str = match geo_obj.get("coordinates").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => match geo_obj.get("coordinates") {
            Some(v) if v.is_array() => &v.to_string(),
            _ => return true,
        },
    };
    let geoproperty = geo_obj
        .get("geoproperty")
        .and_then(|v| v.as_str())
        .unwrap_or(vocab::expanded::LOCATION.as_str());

    let ref_geojson_str = match serde_json::to_string(&serde_json::json!({
        "type": geometry_str,
        "coordinates": serde_json::from_str::<Value>(coordinates_str).unwrap_or(Value::Null)
    })) {
        Ok(s) => s,
        Err(_) => return true,
    };

    let ref_geom: geo::Geometry = match ref_geojson_str.parse::<geojson::GeoJson>() {
        Ok(geojson::GeoJson::Geometry(g)) => match geo::Geometry::try_from(g) {
            Ok(geom) => geom,
            Err(_) => return true,
        },
        _ => return true,
    };

    let entity_geojson = extract_geoproperty_geojson(entity_data, geoproperty);
    let entity_geojson_str = match entity_geojson {
        Some(s) => s,
        None => return false,
    };

    let entity_geom: geo::Geometry = match entity_geojson_str.parse::<geojson::GeoJson>() {
        Ok(geojson::GeoJson::Geometry(g)) => match geo::Geometry::try_from(g) {
            Ok(geom) => geom,
            Err(_) => return false,
        },
        _ => return false,
    };

    let parts: Vec<&str> = georel_str.split(';').collect();
    let rel = parts[0].trim();

    match rel {
        "within" => ref_geom.contains(&entity_geom),
        "contains" => entity_geom.contains(&ref_geom),
        "intersects" => entity_geom.intersects(&ref_geom),
        "disjoint" => !entity_geom.intersects(&ref_geom),
        "equals" => entity_geom.contains(&ref_geom) && ref_geom.contains(&entity_geom),
        "overlaps" => entity_geom.intersects(&ref_geom),
        "near" => {
            let mut max_distance: Option<f64> = None;
            let mut min_distance: Option<f64> = None;
            for part in &parts[1..] {
                let part = part.trim();
                if let Some(val) = part.strip_prefix("maxDistance==") {
                    max_distance = val.parse().ok();
                } else if let Some(val) = part.strip_prefix("minDistance==") {
                    min_distance = val.parse().ok();
                }
            }

            let distance = haversine_distance_m(&entity_geom, &ref_geom);

            let max_ok = max_distance.is_none_or(|max| distance <= max);
            let min_ok = min_distance.is_none_or(|min| distance >= min);
            max_ok && min_ok
        }
        _ => true,
    }
}

fn extract_geoproperty_geojson(entity: &Value, geoproperty_iri: &str) -> Option<String> {
    let obj = match entity {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => None,
    }?;

    let prop_value = obj.get(geoproperty_iri)?;
    let geojson_value = prop_value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|inst| inst.get(vocab::expanded::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"));

    match geojson_value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(val) if val.is_object() => Some(val.to_string()),
        _ => None,
    }
}

fn haversine_distance_m(a: &geo::Geometry, b: &geo::Geometry) -> f64 {
    use geo::algorithm::Centroid;

    let centroid_a = a.centroid().unwrap_or(geo::Point::new(0.0, 0.0));
    let centroid_b = b.centroid().unwrap_or(geo::Point::new(0.0, 0.0));

    let (lat1, lon1) = (centroid_a.y().to_radians(), centroid_a.x().to_radians());
    let (lat2, lon2) = (centroid_b.y().to_radians(), centroid_b.x().to_radians());

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    6_371_008.8 * c
}

/// Evaluate a scope query against entity scopes.
///
/// Supports AND (`;`), OR (`,`), exact match, hierarchical wildcard (`/#`),
/// and single-level wildcard (`/+`).
pub fn matches_scope_filter(scope_q: &str, entity_data: &Value) -> bool {
    let entity_scopes = crate::service::entity::extract_scopes(entity_data);

    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    or_terms.iter().any(|or_term| {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        and_terms.iter().all(|pattern| entity_scopes.iter().any(|scope| scope_matches(pattern, scope)))
    })
}

fn scope_matches(pattern: &str, scope: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/#") {
        if let Some(rest) = scope.strip_prefix(prefix) {
            rest.starts_with('/') && !rest[1..].contains('/')
        } else {
            false
        }
    } else if pattern.contains("/+") {
        let regex_pattern = format!("^{}$", pattern.replace("/+", "/[^/]+"));
        Regex::new(&regex_pattern).is_ok_and(|re| re.is_match(scope))
    } else {
        pattern == scope
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::{
        domain::{
            notification::{ChangeEvent, ChangeOrigin, CompiledSubscription, TriggerReason},
            subscription::SubscriptionStatus,
        },
        renderer::SysAttrsInclusion,
        transport::endpoint::EndpointTransport,
    };

    fn make_sub() -> CompiledSubscription {
        CompiledSubscription {
            id: "urn:ngsi-ld:Subscription:test".to_string(),
            status: SubscriptionStatus::Active,
            endpoint_uri: "http://example.com/notify".to_string(),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_transport: EndpointTransport::Http,
            triggers: vec!["entityCreated".to_string(), "entityUpdated".to_string()],
            watched_attrs: None,
            selectors: Vec::new(),
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            compiled_q: None,
            geo_q: None,
            scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: 1,
            compiled_csf: None,
            local_only: false,
        }
    }

    fn make_event() -> ChangeEvent {
        ChangeEvent::new(
            "urn:ngsi-ld:Entity:001".parse().expect("valid test URI"),
            vec!["Building".to_string()],
            TriggerReason::EntityCreated,
        )
        .with_changed_attributes(vec!["temperature".to_string()])
    }

    #[test]
    fn subscription_matches_without_local_only() {
        let sub = make_sub();
        let event = make_event();
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn subscription_matches_against_local_event() {
        let sub = make_sub();
        let event = make_event();
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn subscription_skips_when_paused() {
        let mut sub = make_sub();
        sub.status = SubscriptionStatus::Paused;
        let event = make_event();
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn subscription_skips_wrong_trigger() {
        let sub = make_sub();
        let event = ChangeEvent::new(
            "urn:ngsi-ld:Entity:001".parse().expect("valid test URI"),
            vec!["Building".to_string()],
            TriggerReason::EntityDeleted,
        );
        assert!(!matches_subscription(&sub, &event));
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=true rejects forwarded events.
    #[test]
    fn local_only_subscription_rejects_forwarded_event() {
        let mut sub = make_sub();
        sub.local_only = true;
        let mut event = make_event();
        event.origin = ChangeOrigin::Forwarded;
        assert!(!matches_subscription(&sub, &event));
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=true still matches local events.
    #[test]
    fn local_only_subscription_accepts_local_event() {
        let mut sub = make_sub();
        sub.local_only = true;
        let event = make_event();
        assert!(matches_subscription(&sub, &event));
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=false allows forwarded events.
    #[test]
    fn non_local_only_subscription_accepts_forwarded_event() {
        let sub = make_sub();
        let mut event = make_event();
        event.origin = ChangeOrigin::Forwarded;
        assert!(matches_subscription(&sub, &event));
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=false allows local events (baseline).
    #[test]
    fn non_local_only_subscription_accepts_local_event() {
        let sub = make_sub();
        let event = make_event();
        assert!(matches_subscription(&sub, &event));
    }
}
