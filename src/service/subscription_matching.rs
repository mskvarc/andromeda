//! Subscription matching and entity-level filtering (geoQ, scopeQ, q).
//!
//! Pure domain logic for determining whether an entity change event matches
//! a compiled subscription. Used by the notification worker (event-driven
//! delivery) and the subscription cache (outbox entry generation).

use crate::service::compiled_subscription::{CompiledGeoQuery, CompiledScopePattern, CompiledScopeTerm, CompiledSubscription};
use andromeda_domain::{
    notification::{ChangeEvent, ChangeOrigin},
    subscription::SubscriptionStatus,
    vocab,
};
use andromeda_geometry::{geometry::Geometry, relation::matches_geo};
use andromeda_query::filter::eval;
use chrono::Utc;
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
                event.entity_types.iter().any(|t| pattern.is_match(t.as_str()))
            } else {
                event.entity_types.iter().any(|t| t.as_str() == sel.type_)
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
        && !event.changed_attributes.iter().any(|a| watched.contains(a.as_str()))
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
        && !eval::evaluate(expr, entity_data)
    {
        debug!("Subscription {} skipped: q filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref geo_q) = sub.compiled_geo_q
        && !matches_compiled_geo_query(geo_q, entity_data)
    {
        debug!("Subscription {} skipped: geoQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref scope_q) = sub.compiled_scope_q
        && !matches_compiled_scope_pattern(scope_q, entity_data)
    {
        debug!("Subscription {} skipped: scopeQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref csf_expr) = sub.compiled_csf
        && !eval::evaluate(csf_expr, entity_data)
    {
        debug!("Subscription {} skipped: csf filter did not match entity", sub.id);
        return false;
    }

    true
}

/// Evaluate a pre-compiled geo-spatial filter against entity data.
///
/// Accepts the [`CompiledGeoQuery`] produced once at subscription compile
/// time so event matching does not re-parse the GeoJSON reference geometry
/// or georel operator.
pub fn matches_compiled_geo_query(geo_q: &CompiledGeoQuery, entity_data: &Value) -> bool {
    let Some(entity_geometry) = extract_geoproperty_geometry(entity_data, &geo_q.geoproperty_iri) else {
        return false;
    };
    matches_geo(&geo_q.georel, entity_geometry.as_ref(), &geo_q.ref_geometry)
}

fn extract_geoproperty_geometry(entity: &Value, geoproperty_iri: &str) -> Option<Geometry> {
    let obj = match entity {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }?;

    let prop_value = obj.get(geoproperty_iri)?;
    let geojson_value = prop_value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|inst| inst.get(vocab::expanded::properties::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"))?;

    match geojson_value {
        // A GeoProperty written as a JSON string carries the GeoJSON document
        // verbatim, which clause 4.7 permits alongside the object form.
        Value::String(s) => {
            let parsed: Value = serde_json::from_str(s).ok()?;
            Geometry::from_geojson_value(&parsed).ok()
        }
        object @ Value::Object(_) => Geometry::from_geojson_value(object).ok(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
    }
}

/// Evaluate a pre-compiled scope query against entity scopes.
///
/// Accepts the [`CompiledScopePattern`] built once at subscription compile
/// time, so event matching does not re-split the pattern string or recompile
/// `/+` regexes on every event.
pub fn matches_compiled_scope_pattern(pattern: &CompiledScopePattern, entity_data: &Value) -> bool {
    let entity_scopes = crate::service::entity::extract_scopes(entity_data);

    pattern.or_terms.iter().any(|and_terms| {
        and_terms
            .iter()
            .all(|term| entity_scopes.iter().any(|scope| compiled_scope_term_matches(term, scope)))
    })
}

fn compiled_scope_term_matches(term: &CompiledScopeTerm, scope: &str) -> bool {
    match term {
        CompiledScopeTerm::Exact(p) => p == scope,
        CompiledScopeTerm::HierarchicalWildcard { prefix } => {
            if let Some(rest) = scope.strip_prefix(prefix.as_str()) {
                rest.starts_with('/') && !rest[1..].contains('/')
            } else {
                false
            }
        }
        CompiledScopeTerm::Regex(re) => re.is_match(scope),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::endpoint::EndpointTransport;
    use andromeda_domain::{
        notification::{ChangeEvent, ChangeOrigin, TriggerReason},
        subscription::SubscriptionStatus,
        sys_attrs::SysAttrsInclusion,
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
            compiled_geo_q: None,
            compiled_scope_q: None,
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
            vec!["Building".parse().expect("valid test type name")],
            TriggerReason::EntityCreated,
        )
        .with_changed_attributes(vec!["temperature".parse().expect("valid test attribute name")])
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
            vec!["Building".parse().expect("valid test type name")],
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
