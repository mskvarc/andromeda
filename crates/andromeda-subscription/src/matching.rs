//! Subscription matching and entity-level filtering (geoQ, scopeQ, q).
//!
//! Pure domain logic for determining whether an entity change event matches
//! a compiled subscription. Used by the notification worker (event-driven
//! delivery) and the subscription cache (notification-event generation).

use crate::compiled::{CompiledCsrSubscription, CompiledSubscription};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    expanded::query_match::{matches_geo_query, matches_scope_query},
    notification::ChangeOrigin,
    subscription::SubscriptionStatus,
};
use andromeda_ngsi_ld_query::filter::eval;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// Match a context-source subscription against one registration representation (clause 5.12).
pub fn matches_csource_subscription(sub: &CompiledCsrSubscription, registration: &Value) -> bool {
    if sub.status != SubscriptionStatus::Active {
        return false;
    }
    if sub.expires_at.is_some_and(|expires_at| expires_at <= Utc::now()) || sub.time_interval_secs.is_some() {
        return false;
    }
    if !sub.selectors.is_empty() {
        let information = registration.get("information").and_then(Value::as_array);
        let selector_matches = sub.selectors.iter().any(|selector| {
            information.is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry.get("entities").and_then(Value::as_array).is_some_and(|entities| {
                        entities.iter().any(|entity| {
                            let entity_type = entity.get("type").and_then(Value::as_str).unwrap_or("");
                            selector.type_.matches(&[entity_type])
                                && selector
                                    .id
                                    .as_ref()
                                    .is_none_or(|id| entity.get("id").and_then(Value::as_str) == Some(id.as_ref()))
                                && selector
                                    .id_pattern
                                    .as_ref()
                                    .is_none_or(|pattern| entity.get("id").and_then(Value::as_str).is_some_and(|id| pattern.is_match(id)))
                        })
                    })
                })
            })
        });
        if !selector_matches {
            return false;
        }
    }
    if let Some(watched) = &sub.watched_attrs {
        let attribute_matches = registration.get("information").and_then(Value::as_array).is_some_and(|entries| {
            entries.iter().any(|entry| {
                ["propertyNames", "relationshipNames"].iter().any(|member| {
                    entry.get(*member).and_then(Value::as_array).is_some_and(|names| {
                        names
                            .iter()
                            .filter_map(Value::as_str)
                            .filter_map(|raw| raw.parse::<AttributeName>().ok())
                            .any(|name| watched.contains(&name))
                    })
                })
            })
        });
        if !attribute_matches {
            return false;
        }
    }
    sub.compiled_csf.as_ref().is_none_or(|expression| eval::evaluate(expression, registration))
}

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

    // A subscription is notified of changes that happen while it is active, not
    // of the state that existed before it (clause 5.2.12: a subscription has no
    // initial notification). An entity change committed before the subscription
    // was created can still reach the notification worker after the subscription
    // has entered the cache; matching it here would deliver a notification for a
    // change the subscriber could never have asked to observe.
    if event.occurred_at < sub.created_at {
        debug!(
            "Subscription {} skipped: event occurred at {} before the subscription was created at {}",
            sub.id, event.occurred_at, sub.created_at
        );
        return false;
    }

    if sub.time_interval_secs.is_some() {
        return false;
    }

    if sub.local_only && event.origin == ChangeOrigin::Forwarded {
        debug!("Subscription {} skipped: localOnly=true but event origin is forwarded", sub.id);
        return false;
    }

    // The compiled triggers and the event carry the same typed vocabulary, so
    // membership is an enum comparison rather than the per-event string
    // allocation this used to make (Table 5.2.12).
    if !sub.triggers.contains(&event.trigger_reason) {
        debug!("Subscription {} skipped: trigger '{}' not in {:?}", sub.id, event.trigger_reason, sub.triggers);
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
        && !event.changed_attributes.iter().any(|attribute| watched.contains(attribute))
    {
        debug!(
            "Subscription {} skipped: watchedAttributes {:?} did not match event attrs {:?}",
            sub.id, watched, event.changed_attributes
        );
        return false;
    }

    debug!(
        "Subscription {} matched event {} (trigger={}, types={:?}, attrs={:?})",
        sub.id, event.entity_id, event.trigger_reason, event.entity_types, event.changed_attributes
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compilation::compile_geo_q, error::SubscriptionError, subscription_key::SubscriptionKey};
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_ngsi_ld_model::{notification::NotificationTrigger, subscription::SubscriptionStatus};
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        geo::{compile::compile_geo_query, input::GeoQueryInput},
        term_expander::TermExpander,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::DateTime;
    use futures::executor::block_on;
    use iri_rs::IriBuf;
    use std::{fmt::Display, num::NonZeroU32, str::FromStr};
    use url::Url;

    fn parse_fixture<T>(value: &str, description: &str) -> Result<T, SubscriptionError>
    where
        T: FromStr,
        T::Err: Display,
    {
        value
            .parse()
            .map_err(|error| SubscriptionError::internal(format!("invalid {description} fixture: {error}")))
    }

    fn make_sub() -> Result<CompiledSubscription, SubscriptionError> {
        Ok(CompiledSubscription {
            id: parse_fixture("urn:ngsi-ld:Subscription:test", "subscription URI")?,
            key: SubscriptionKey::try_from(1).map_err(|error| SubscriptionError::internal(format!("invalid subscription key fixture: {error}")))?,
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").map_err(|error| SubscriptionError::internal(format!("invalid endpoint fixture: {error}")))?,
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            triggers: vec![NotificationTrigger::EntityCreated, NotificationTrigger::EntityUpdated],
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
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
        })
    }

    fn make_event() -> Result<ChangeEvent, SubscriptionError> {
        Ok(ChangeEvent::builder()
            .entity_id(parse_fixture("urn:ngsi-ld:Entity:001", "entity URI")?)
            .entity_types(vec![parse_fixture("Building", "entity type")?])
            .trigger_reason(NotificationTrigger::EntityCreated)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .changed_attributes(vec![parse_fixture("temperature", "attribute name")?])
            .build())
    }

    fn at(unix_seconds: i64) -> Result<DateTime<Utc>, SubscriptionError> {
        DateTime::from_timestamp(unix_seconds, 0).ok_or_else(|| SubscriptionError::internal("fixture timestamp out of range".to_owned()))
    }

    /// Clause 5.2.12: a subscription has no initial notification, so a change
    /// that occurred before the subscription was created never notifies it. The
    /// notification worker can still see such a change after the subscription has
    /// entered the cache, which is when this guard has to hold.
    #[test]
    fn a_change_that_occurred_before_the_subscription_was_created_is_not_matched() -> Result<(), SubscriptionError> {
        let mut event = make_event()?;
        event.occurred_at = at(1_000)?;
        let mut sub = make_sub()?;

        sub.created_at = at(2_000)?;
        assert!(
            !matches_subscription(&sub, &event),
            "a change before the subscription's creation must not match"
        );

        sub.created_at = at(1_000)?;
        assert!(
            matches_subscription(&sub, &event),
            "a change at the subscription's creation instant still matches"
        );

        sub.created_at = at(500)?;
        assert!(matches_subscription(&sub, &event), "a change after the subscription's creation matches");
        Ok(())
    }

    fn make_csource_sub() -> Result<CompiledCsrSubscription, SubscriptionError> {
        Ok(CompiledCsrSubscription {
            id: parse_fixture("urn:ngsi-ld:Subscription:csource", "CSR subscription URI")?,
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").map_err(|error| SubscriptionError::internal(format!("invalid endpoint fixture: {error}")))?,
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            selectors: Vec::new(),
            watched_attrs: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
            expires_at: None,
            compiled_csf: None,
        })
    }

    /// Resolves every term to the NGSI-LD `location` GeoProperty, which is what
    /// the core context does for the default `geoproperty`.
    struct LocationExpander;

    impl TermExpander for LocationExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            IriBuf::new(LOCATION.to_owned()).map_err(|error| QueryError::TermExpansion {
                term: term.to_owned(),
                message: error.to_string(),
            })
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            IriBuf::new(format!("https://example.com/{term}")).map_err(|error| QueryError::TermExpansion {
                term: term.to_owned(),
                message: error.to_string(),
            })
        }
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn located(longitude: f64, latitude: f64) -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [longitude, latitude]}}]}]
        }])
    }

    #[test]
    fn a_subscription_geo_filter_is_the_same_compiled_type_a_query_uses() -> Result<(), SubscriptionError> {
        // Clause 4.10 is one relation, so a subscription's stored `geoQ` and a
        // request's `georel`/`geometry`/`coordinates` must compile to the same
        // value. They used to be two types, one of them stringly-typed, and
        // only one of them had an in-memory evaluator.
        let from_subscription = compile_geo_q(
            &serde_json::json!({
                "georel": "near;maxDistance==1000",
                "geometry": "Point",
                "coordinates": [13.4, 52.5],
                "geoproperty": LOCATION
            }),
            "urn:ngsi-ld:Subscription:test",
        )
        .ok_or_else(|| SubscriptionError::internal("the subscription geoQ fixture did not compile"))?;

        let request_geo = GeoQueryInput::from_wire(
            "near;maxDistance==1000".to_string(),
            "Point".to_string(),
            serde_json::json!([13.4, 52.5]),
            Some(parse_fixture("location", "geoproperty")?),
        )
        .map_err(|error| SubscriptionError::validation(error.to_string()))?;
        let from_request = block_on(compile_geo_query(Some(&request_geo), &LocationExpander))?
            .ok_or_else(|| SubscriptionError::internal("the complete request geo fixture did not compile"))?;

        pretty_assertions::assert_eq!(from_subscription, from_request);
        Ok(())
    }

    #[test]
    fn a_subscription_geo_filter_accepts_only_entities_inside_the_relation() -> Result<(), SubscriptionError> {
        let geo_query = compile_geo_q(
            &serde_json::json!({
                "georel": "near;maxDistance==1000",
                "geometry": "Point",
                "coordinates": [13.4, 52.5],
                "geoproperty": LOCATION
            }),
            "urn:ngsi-ld:Subscription:test",
        )
        .ok_or_else(|| SubscriptionError::internal("the subscription geoQ fixture did not compile"))?;

        assert!(matches_geo_query(&geo_query, &located(13.4, 52.5)));
        assert!(!matches_geo_query(&geo_query, &located(13.4, 53.5)));
        assert!(!matches_geo_query(&geo_query, &serde_json::json!([{"@id": "urn:ngsi-ld:Building:002"}])));
        Ok(())
    }

    #[test]
    fn subscription_matches_without_local_only() -> Result<(), SubscriptionError> {
        let sub = make_sub()?;
        let event = make_event()?;
        assert!(matches_subscription(&sub, &event));
        Ok(())
    }

    #[test]
    fn subscription_matches_against_local_event() -> Result<(), SubscriptionError> {
        let sub = make_sub()?;
        let event = make_event()?;
        assert!(matches_subscription(&sub, &event));
        Ok(())
    }

    #[test]
    fn subscription_skips_when_paused() -> Result<(), SubscriptionError> {
        let mut sub = make_sub()?;
        sub.status = SubscriptionStatus::Paused;
        let event = make_event()?;
        assert!(!matches_subscription(&sub, &event));
        Ok(())
    }

    #[test]
    fn subscription_skips_wrong_trigger() -> Result<(), SubscriptionError> {
        let sub = make_sub()?;
        let event = ChangeEvent::new(
            parse_fixture("urn:ngsi-ld:Entity:001", "entity URI")?,
            vec![parse_fixture("Building", "entity type")?],
            NotificationTrigger::EntityDeleted,
        );
        assert!(!matches_subscription(&sub, &event));
        Ok(())
    }

    #[test]
    fn active_csource_subscription_without_filters_matches_a_registration() -> Result<(), SubscriptionError> {
        assert!(matches_csource_subscription(&make_csource_sub()?, &serde_json::json!({"information": []})));
        Ok(())
    }

    #[test]
    fn paused_csource_subscription_does_not_match_a_registration() -> Result<(), SubscriptionError> {
        let mut subscription = make_csource_sub()?;
        subscription.status = SubscriptionStatus::Paused;
        assert!(!matches_csource_subscription(&subscription, &serde_json::json!({"information": []})));
        Ok(())
    }

    #[test]
    fn interval_csource_subscription_is_excluded_from_event_matching() -> Result<(), SubscriptionError> {
        let mut subscription = make_csource_sub()?;
        subscription.time_interval_secs = Some(10.0);
        assert!(!matches_csource_subscription(&subscription, &serde_json::json!({"information": []})));
        Ok(())
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=true rejects forwarded events.
    #[test]
    fn local_only_subscription_rejects_forwarded_event() -> Result<(), SubscriptionError> {
        let mut sub = make_sub()?;
        sub.local_only = true;
        let mut event = make_event()?;
        event.origin = ChangeOrigin::Forwarded;
        assert!(!matches_subscription(&sub, &event));
        Ok(())
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=true still matches local events.
    #[test]
    fn local_only_subscription_accepts_local_event() -> Result<(), SubscriptionError> {
        let mut sub = make_sub()?;
        sub.local_only = true;
        let event = make_event()?;
        assert!(matches_subscription(&sub, &event));
        Ok(())
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=false allows forwarded events.
    #[test]
    fn non_local_only_subscription_accepts_forwarded_event() -> Result<(), SubscriptionError> {
        let sub = make_sub()?;
        let mut event = make_event()?;
        event.origin = ChangeOrigin::Forwarded;
        assert!(matches_subscription(&sub, &event));
        Ok(())
    }

    /// ETSI GS CIM 009 v1.9.1, clause 5.5.13: localOnly=false allows local events (baseline).
    #[test]
    fn non_local_only_subscription_accepts_local_event() -> Result<(), SubscriptionError> {
        let sub = make_sub()?;
        let event = make_event()?;
        assert!(matches_subscription(&sub, &event));
        Ok(())
    }
}
