//! Subscription matching and entity-level filtering (geoQ, scopeQ, q).
//!
//! Pure domain logic for determining whether an entity change event matches
//! a compiled subscription. Used by the notification worker (event-driven
//! delivery) and the subscription cache (outbox entry generation).

use crate::service::{compiled_subscription::CompiledSubscription, geo_filter::matches_geo_query};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_model::{
    expanded::entity_member::extract_scopes,
    notification::{ChangeEvent, ChangeOrigin},
    subscription::SubscriptionStatus,
};
use andromeda_ngsi_ld_query::{filter::eval, scope::query::ScopeQuery};
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
        let entity_id_str = event.entity_id.to_string(); // convert once for the pattern comparison below
        let any_selector_matches = sub.selectors.iter().any(|sel| {
            let entity_types: Vec<&str> = event.entity_types.iter().map(EntityTypeName::as_str).collect();
            if !sel.type_.matches(&entity_types) {
                debug!(
                    "Subscription {} selector type mismatch: selector={} event_types={:?}",
                    sub.id, sel.type_, event.entity_types
                );
                return false;
            }

            if let Some(ref id) = sel.id
                && id != &event.entity_id
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
        && !matches_geo_query(geo_q, entity_data)
    {
        debug!("Subscription {} skipped: geoQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref scope_q) = sub.compiled_scope_q
        && !matches_scope_query(scope_q, entity_data)
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

/// Evaluate a parsed `scopeQ` against the scopes an entity carries.
///
/// The clause 4.18 semantics live on [`ScopeQuery`]; this reads the scopes off
/// the expanded payload and asks it. An earlier local reading here had `#`
/// match exactly one further level, where the stored query and its `lquery`
/// rendering match one or more.
pub fn matches_scope_query(query: &ScopeQuery, entity_data: &Value) -> bool {
    query.matches(&extract_scopes(entity_data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_delivery::endpoint::transport::EndpointTransport;
    use andromeda_ngsi_ld_model::{
        notification::{ChangeEvent, ChangeOrigin, TriggerReason},
        subscription::SubscriptionStatus,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

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

    /// Resolves every term to the NGSI-LD `location` GeoProperty, which is what
    /// the core context does for the default `geoproperty`.
    struct LocationExpander;

    impl andromeda_ngsi_ld_query::term_expander::TermExpander for LocationExpander {
        async fn expand_term(&self, _term: &str) -> Result<iri_rs::IriBuf, andromeda_ngsi_ld_query::error::QueryError> {
            Ok(iri_rs::IriBuf::new(LOCATION.to_string()).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<iri_rs::IriBuf, andromeda_ngsi_ld_query::error::QueryError> {
            Ok(iri_rs::IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn located(longitude: f64, latitude: f64) -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [longitude, latitude]}}]}]
        }])
    }

    #[tokio::test]
    async fn a_subscription_geo_filter_is_the_same_compiled_type_a_query_uses() {
        // Clause 4.10 is one relation, so a subscription's stored `geoQ` and a
        // request's `georel`/`geometry`/`coordinates` must compile to the same
        // value. They used to be two types, one of them stringly-typed, and
        // only one of them had an in-memory evaluator.
        let from_subscription = crate::service::subscription_cache::compile_geo_q(
            &serde_json::json!({
                "georel": "near;maxDistance==1000",
                "geometry": "Point",
                "coordinates": [13.4, 52.5],
                "geoproperty": LOCATION
            }),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("the subscription geoQ compiles");

        let from_request = andromeda_ngsi_ld_query::geo::compile::compile_geo_query(
            Some("near;maxDistance==1000"),
            Some("Point"),
            Some("[13.4,52.5]"),
            Some("location"),
            &LocationExpander,
        )
        .await
        .expect("the request geo query compiles")
        .expect("all three parameters were present");

        pretty_assertions::assert_eq!(from_subscription, from_request);
    }

    #[test]
    fn a_subscription_geo_filter_and_a_residual_geo_filter_give_the_same_verdict() {
        // The subscription path and the residual path share one reading of
        // clause 4.10, so a geometry either satisfies both or neither.
        let geo_query = crate::service::subscription_cache::compile_geo_q(
            &serde_json::json!({
                "georel": "near;maxDistance==1000",
                "geometry": "Point",
                "coordinates": [13.4, 52.5],
                "geoproperty": LOCATION
            }),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("the subscription geoQ compiles");

        let residual = crate::service::entity::residual_filter::ResidualFilter::over(
            andromeda_ngsi_ld_query::entity_selection::EntitySelection::builder()
                .geo(Some(geo_query.clone())) // clone: the residual owns its predicate and the direct evaluator borrows this one
                .build(),
        );

        for entity in [
            located(13.4, 52.5),
            located(13.4, 53.5),
            serde_json::json!([{"@id": "urn:ngsi-ld:Building:002"}]),
        ] {
            pretty_assertions::assert_eq!(
                matches_geo_query(&geo_query, &entity),
                residual.matches(&entity),
                "the two readings disagreed on {entity}"
            );
        }
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
