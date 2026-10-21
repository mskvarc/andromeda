//! Matching a context source registration subscription against a registration
//! (ETSI GS CIM 009 clauses 5.11.2.4 and 5.12).
//!
//! The entity selectors and the attribute identifiers match the registration's
//! `information` as clause 5.12 describes; the `temporalQ`, `geoQ`, `scopeQ`
//! and `csf` match the registration's own members as clause 5.11.2.4 describes.

use crate::compiled::CompiledCsrSubscription;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_model::{
    registration::{ContextSourceRegistration, context_source_properties::ContextSourceProperties, info::RegistrationInfo},
    registration_matching::{EntitySpecification, RegistrationMatchSpecification},
    subscription::SubscriptionStatus,
};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, filter::eval::evaluate, queried_entity::linked_entities::NoLinkedEntities};
use chrono::Utc;

/// Whether registration changes notify `sub` at all: the subscription is
/// live and change-driven.
///
/// Clause 5.11.2.4 ties a `timeInterval` subscription's notifications to its
/// schedule alone, "independent of any changes to the set of Context Source
/// registrations", so a change reaches only the subscriptions without one.
/// Whether the changed registration concerns the subscription is
/// [`csource_subscription_selects_registration`].
#[must_use]
pub fn csource_subscription_follows_registration_changes(sub: &CompiledCsrSubscription) -> bool {
    csource_subscription_is_live(sub) && sub.time_interval_secs.is_none()
}

/// Whether `sub` may be notified at all: it is active and has not reached its
/// `expiresAt` (Table 5.2.12-2, clause 5.11.7).
///
/// The cadence is a separate question -- this is the lifecycle one both
/// cadences ask first.
#[must_use]
pub fn csource_subscription_is_live(sub: &CompiledCsrSubscription) -> bool {
    sub.status == SubscriptionStatus::Active && sub.expires_at.is_none_or(|expires_at| expires_at > Utc::now())
}

/// Whether `registration` is one of the registrations `sub` asks about,
/// irrespective of the subscription's lifecycle (clauses 5.11.2.4 and 5.12).
///
/// This is the reading both a change-driven notification and a periodic
/// `timeInterval` notification (clause 5.11.7) use.
pub fn csource_subscription_selects_registration(sub: &CompiledCsrSubscription, registration: &ContextSourceRegistration) -> bool {
    let entities: Vec<EntitySpecification<'_>> = sub.selectors.iter().map(EntitySpecification::from_selector).collect();
    let specification = RegistrationMatchSpecification {
        entities: &entities,
        attributes: &sub.matching_attrs,
    };
    registration.coverage.information.iter().any(|info| specification.matches_information(info))
        && temporal_partition_admits(sub.temporal_q.as_ref(), registration)
        && sub.geo_q.admits(|geo_q| registration.coverage.admits_geo_query(geo_q))
        && scope_query_admits(sub, registration)
        && context_source_filter_admits(sub, registration)
}

/// The `RegistrationInfo` elements of `registration` that `sub` matches, in
/// order: what a notification carries instead of the whole `information`
/// (clause 5.11.7).
pub fn matching_information<'registration>(
    sub: &CompiledCsrSubscription,
    registration: &'registration ContextSourceRegistration,
) -> Vec<&'registration RegistrationInfo> {
    let entities: Vec<EntitySpecification<'_>> = sub.selectors.iter().map(EntitySpecification::from_selector).collect();
    let specification = RegistrationMatchSpecification {
        entities: &entities,
        attributes: &sub.matching_attrs,
    };
    specification.matching_information(&registration.coverage.information).collect()
}

/// Clause 5.11.2.4: without a `temporalQ` only registrations declaring no
/// interval are considered; with one, the interval the `timeproperty`
/// selects must be present and meet the window.
fn temporal_partition_admits(temporal_q: Option<&CompiledCsrTemporalFilter>, registration: &ContextSourceRegistration) -> bool {
    let coverage = &registration.coverage;
    let Some(temporal_q) = temporal_q else {
        return coverage.observation_interval.is_none() && coverage.management_interval.is_none();
    };
    let relevant = match temporal_q.timeproperty.unwrap_or(TimeProp::Observed) {
        TimeProp::Observed => coverage.observation_interval.as_ref(),
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => coverage.management_interval.as_ref(),
    };
    relevant.is_some_and(|interval| interval.meets(&temporal_q.interval))
}

/// Clause 5.11.2.4: the scope query is matched against the registration's
/// `scope`; a registration declaring none has nothing a pattern could match.
fn scope_query_admits(sub: &CompiledCsrSubscription, registration: &ContextSourceRegistration) -> bool {
    let Some(scope_q) = &sub.scope_q else {
        return true;
    };
    registration.coverage.scope.as_deref().is_some_and(|scopes| scope_q.matches(scopes))
}

/// Clause 5.11.2.4: the context source filter is matched against the
/// registration's Context Source Properties (clause 4.9), which carry no
/// Relationship a hop could cross.
fn context_source_filter_admits(sub: &CompiledCsrSubscription, registration: &ContextSourceRegistration) -> bool {
    let Some(csf) = &sub.compiled_csf else {
        return true;
    };
    evaluate(
        csf,
        ContextSourceProperties::of_registration(&registration.additional_properties),
        NoLinkedEntities,
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        compiled::CompiledCsrSubscription,
        csource_matching::{csource_subscription_follows_registration_changes, csource_subscription_selects_registration, matching_information},
        geo_filter::SubscriptionGeoFilter,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{
        registration::ContextSourceRegistration,
        subscription::{EntitySelector, SubscriptionStatus},
    };
    use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, filter::parser, scope::query::ScopeQuery, temporal::window::TimeInterval};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use url::Url;

    const VEHICLE: &str = "https://example.com/Vehicle";
    const SPEED: &str = "https://example.com/speed";
    const TIER: &str = "https://uri.etsi.org/ngsi-ld/default-context/tier";

    /// Whether a change to `registration` notifies `sub`.
    fn notified_of_change(sub: &CompiledCsrSubscription, registration: &ContextSourceRegistration) -> bool {
        csource_subscription_follows_registration_changes(sub) && csource_subscription_selects_registration(sub, registration)
    }

    fn subscription() -> CompiledCsrSubscription {
        CompiledCsrSubscription {
            id: "urn:ngsi-ld:Subscription:csource".parse().expect("a valid test fixture"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("a valid test fixture"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: Vec::new(),
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: SubscriptionGeoFilter::Unrestricted,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
            expires_at: None,
            compiled_csf: None,
        }
    }

    fn selector(raw_type: &str) -> EntitySelector {
        serde_json::from_value(json!({"type": raw_type})).expect("selector parses")
    }

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("valid attribute name")
    }

    fn registration(extra: &Value) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}],
            "endpoint": "https://source.example.com/",
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        });
        if let (Some(target), Some(source)) = (document.as_object_mut(), extra.as_object()) {
            target.extend(source.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(document).expect("registration parses")
    }

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    fn temporal_q(interval: TimeInterval, timeproperty: Option<TimeProp>) -> CompiledCsrTemporalFilter {
        CompiledCsrTemporalFilter::builder().interval(interval).timeproperty(timeproperty).build()
    }

    #[test]
    fn an_active_subscription_without_filters_matches_and_a_paused_or_interval_one_does_not() {
        let registration = registration(&json!({}));
        assert!(notified_of_change(&subscription(), &registration));
        let mut paused = subscription();
        paused.status = SubscriptionStatus::Paused;
        assert!(!notified_of_change(&paused, &registration));
        let mut periodic = subscription();
        periodic.time_interval_secs = Some(10.0);
        assert!(!notified_of_change(&periodic, &registration));
        assert!(
            csource_subscription_selects_registration(&periodic, &registration),
            "the periodic worker still asks whether the registration is selected"
        );
    }

    /// Clause 5.12: the selectors and the attribute identifiers match the
    /// registration's `information`.
    #[test]
    fn entity_selectors_and_matching_attributes_are_matched_against_information() {
        let mut sub = subscription();
        sub.selectors = vec![selector(VEHICLE)];
        sub.matching_attrs = vec![name(SPEED)];
        assert!(notified_of_change(&sub, &registration(&json!({}))));
        sub.selectors = vec![selector("https://example.com/Building")];
        assert!(!notified_of_change(&sub, &registration(&json!({}))));
        sub.selectors = vec![selector(VEHICLE)];
        sub.matching_attrs = vec![name("https://example.com/brandName")];
        assert!(!notified_of_change(&sub, &registration(&json!({}))));
        let entity_only = registration(&json!({"information": [{"entities": [{"type": VEHICLE}]}]}));
        assert!(notified_of_change(&sub, &entity_only), "only entities registered: any attribute may be held");
    }

    #[test]
    fn matching_information_trims_to_the_elements_the_subscription_matches() {
        let mut sub = subscription();
        sub.selectors = vec![selector(VEHICLE)];
        let registration = registration(&json!({"information": [
            {"entities": [{"type": VEHICLE}]},
            {"entities": [{"type": "https://example.com/Building"}]}
        ]}));
        let kept = matching_information(&sub, &registration);
        pretty_assertions::assert_eq!(kept.len(), 1);
    }

    /// Clause 5.11.2.4: without a `temporalQ` only latest-information
    /// registrations are considered; with one only interval-bearing ones.
    #[test]
    fn the_temporal_partition_table() {
        let latest = registration(&json!({}));
        let observed = registration(&json!({"observationInterval": {"startAt": "2020-01-01T00:00:00Z", "endAt": "2021-01-01T00:00:00Z"}}));
        let managed = registration(&json!({"managementInterval": {"startAt": "2020-01-01T00:00:00Z", "endAt": "2021-01-01T00:00:00Z"}}));
        let without = subscription();
        assert!(notified_of_change(&without, &latest));
        assert!(!notified_of_change(&without, &observed));
        assert!(!notified_of_change(&without, &managed));

        let mut after_observed = subscription();
        after_observed.temporal_q = Some(temporal_q(TimeInterval::After(at("2020-06-01T00:00:00Z")), None));
        assert!(!notified_of_change(&after_observed, &latest));
        assert!(notified_of_change(&after_observed, &observed));
        assert!(!notified_of_change(&after_observed, &managed), "the relevant interval is absent");

        let mut after_created = subscription();
        after_created.temporal_q = Some(temporal_q(TimeInterval::After(at("2020-06-01T00:00:00Z")), Some(TimeProp::Created)));
        assert!(notified_of_change(&after_created, &managed));
        assert!(!notified_of_change(&after_created, &observed));

        let mut outside = subscription();
        outside.temporal_q = Some(temporal_q(TimeInterval::After(at("2022-06-01T00:00:00Z")), None));
        assert!(!notified_of_change(&outside, &observed));

        let mut overlapping = subscription();
        overlapping.temporal_q = Some(temporal_q(
            TimeInterval::Between {
                from: at("2020-06-01T00:00:00Z"),
                to: at("2020-09-01T00:00:00Z"),
            },
            None,
        ));
        assert!(notified_of_change(&overlapping, &observed));
    }

    /// Clause 5.11.2.4: the geoquery is matched against `location` by default;
    /// a registration without one may still cover the area.
    #[test]
    fn the_geoquery_is_matched_against_the_registration_location() {
        let mut sub = subscription();
        let stored =
            serde_json::from_value(json!({"georel": "near;maxDistance==3000", "geometry": "Point", "coordinates": [13.404, 52.520]})).expect("a readable geoQ");
        sub.geo_q = SubscriptionGeoFilter::from_stored(Some(&stored), "urn:ngsi-ld:Subscription:csource");
        let berlin = registration(&json!({"location": {"type": "Point", "coordinates": [13.404, 52.520]}}));
        let paris = registration(&json!({"location": {"type": "Point", "coordinates": [2.349, 48.853]}}));
        assert!(notified_of_change(&sub, &berlin));
        assert!(!notified_of_change(&sub, &paris));
        assert!(notified_of_change(&sub, &registration(&json!({}))), "no location declared: possibly contains");
    }

    /// Clause 5.11.2.4 with clause 5.8.6: a stored `geoQ` that no longer
    /// compiles is still a defined geoquery, so it selects no registration,
    /// not even one that declares no location.
    #[test]
    fn a_stored_geo_q_that_no_longer_compiles_selects_no_registration() {
        let mut sub = subscription();
        let stored = serde_json::from_value(json!({"georel": "near;maxDistance==3000", "geometry": "Point", "coordinates": "[13.404, "}))
            .expect("a readable geoQ whose string-encoded coordinates are no JSON");
        sub.geo_q = SubscriptionGeoFilter::from_stored(Some(&stored), "urn:ngsi-ld:Subscription:csource");
        assert!(!notified_of_change(&sub, &registration(&json!({}))));
        assert!(!notified_of_change(
            &sub,
            &registration(&json!({"location": {"type": "Point", "coordinates": [13.404, 52.520]}}))
        ));
    }

    #[test]
    fn the_scope_query_is_matched_against_the_registration_scope() {
        let mut sub = subscription();
        sub.scope_q = Some(ScopeQuery::parse("/Madrid/#").expect("a valid test fixture"));
        assert!(notified_of_change(&sub, &registration(&json!({"scope": ["/Madrid/Gardens"]}))));
        assert!(!notified_of_change(&sub, &registration(&json!({"scope": ["/Berlin/Parks"]}))));
        assert!(!notified_of_change(&sub, &registration(&json!({}))));
    }

    /// Clause 4.9: `csf` reads the registration's Context Source Properties.
    #[test]
    fn the_context_source_filter_is_matched_against_the_registration_properties() {
        let mut sub = subscription();
        sub.compiled_csf = Some(parser::parse(&format!("\"{TIER}\"==\"gold\"")).expect("a valid test fixture"));
        let gold = registration(&json!({TIER: {"type": "Property", "value": "gold"}}));
        let silver = registration(&json!({TIER: {"type": "Property", "value": "silver"}}));
        assert!(notified_of_change(&sub, &gold));
        assert!(!notified_of_change(&sub, &silver));
        assert!(!notified_of_change(&sub, &registration(&json!({}))));
    }
}
