//! Matching a context source registration subscription against a registration
//! (ETSI GS CIM 009 clauses 5.11.2.4 and 5.12).
//!
//! The entity selectors and the attribute identifiers match the registration's
//! `information` as clause 5.12 describes; the `temporalQ`, `geoQ`, `scopeQ`
//! and `csf` match the registration's own members as clause 5.11.2.4 describes.

use crate::{compiled::CompiledCsrSubscription, geo_filter::SubscriptionGeoFilter};
use andromeda_ngsi_ld_model::{
    registration::{
        ContextSourceRegistration,
        info::RegistrationInfo,
        selection::{RegistrationFilters, ScopeFilter, TemporalPartition},
    },
    registration_matching::{entity_specification::EntitySpecification, match_specification::RegistrationMatchSpecification},
};
use chrono::Utc;
use tracing::warn;

/// Whether registration changes notify `sub` at all: the subscription is
/// notified as it now stands, and is change-driven.
///
/// Clause 5.11.2.4 ties a `timeInterval` subscription's notifications to its
/// schedule alone, "independent of any changes to the set of Context Source
/// registrations", so a change reaches only the subscriptions without one.
/// Whether the changed registration concerns the subscription is
/// [`csource_subscription_selects_registration`].
#[must_use]
pub fn csource_subscription_follows_registration_changes(sub: &CompiledCsrSubscription) -> bool {
    sub.standing.notifies_at(Utc::now()) && sub.time_interval_secs.is_none()
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
    // Clause 5.8.6: a stored geoquery that no longer compiles is still a
    // defined geoquery, so it selects no registration.
    let geo = match &sub.geo_q {
        SubscriptionGeoFilter::Unusable => return false,
        SubscriptionGeoFilter::Unrestricted | SubscriptionGeoFilter::Restricts(_) => sub.geo_q.restriction(),
    };
    let filters = RegistrationFilters {
        scope: sub.scope_q.as_ref().map_or(ScopeFilter::Unfiltered, ScopeFilter::DeclaredScope),
        geo,
        csf: sub.compiled_csf.as_ref(),
        temporal: TemporalPartition::Applied(sub.temporal_q.as_ref()),
    };
    specification.covers(registration)
        && filters.admits(registration).unwrap_or_else(|error| {
            warn!(subscription = %sub.id, registration = %registration.id, %error, "the Context Source Properties of a registration could not be read for the context source filter");
            false
        })
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
        subscription::{SubscriptionStatus, entity_selector::EntitySelector, standing::SubscriptionStanding},
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
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
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
            target.extend(source.iter().map(|(key, value)| (key.clone(), value.clone())));
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
        paused.standing.status = SubscriptionStatus::Paused;
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

    /// Clause 5.11.2.4: a subscription to registrations applies the temporal
    /// partition, latest-information registrations without a `temporalQ` and
    /// interval-bearing ones with it.
    #[test]
    fn a_subscription_applies_the_temporal_partition() {
        let latest = registration(&json!({}));
        let observed = registration(&json!({"observationInterval": {"startAt": "2020-01-01T00:00:00Z", "endAt": "2021-01-01T00:00:00Z"}}));
        let without = subscription();
        assert!(notified_of_change(&without, &latest));
        assert!(!notified_of_change(&without, &observed));

        let mut after = subscription();
        after.temporal_q = Some(temporal_q(TimeInterval::After(at("2020-06-01T00:00:00Z")), None));
        assert!(!notified_of_change(&after, &latest));
        assert!(notified_of_change(&after, &observed));

        let mut after_created = subscription();
        after_created.temporal_q = Some(temporal_q(TimeInterval::After(at("2020-06-01T00:00:00Z")), Some(TimeProp::Created)));
        assert!(!notified_of_change(&after_created, &observed), "the relevant interval is absent");
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

    /// Clause 4.9 targets "a Context Source Property member of the
    /// `CSourceRegistration`, see Table 5.2.9-1", so `mode` is one, and a
    /// registration declaring none is `inclusive`.
    #[test]
    fn the_context_source_filter_reads_the_registration_mode() {
        let mut sub = subscription();
        sub.compiled_csf = Some(parser::parse("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"").expect("a valid test fixture"));
        assert!(notified_of_change(&sub, &registration(&json!({"mode": "exclusive"}))));
        assert!(!notified_of_change(&sub, &registration(&json!({"mode": "inclusive"}))));
        sub.compiled_csf = Some(parser::parse("\"https://uri.etsi.org/ngsi-ld/mode\"==\"inclusive\"").expect("a valid test fixture"));
        assert!(notified_of_change(&sub, &registration(&json!({}))));
    }

    /// Clause 4.9 with Table 5.2.9-1: each defined member is read under the
    /// name the core context expands it to, `expiresAt` as a `DateTime`, and a
    /// Table 5.2.9-2 delivery statistic is no target.
    #[test]
    fn the_context_source_filter_reads_each_defined_member_and_no_delivery_statistic() {
        let declared = registration(&json!({
            "registrationName": "vehicles",
            "description": "vehicle telemetry source",
            "management": {"timeout": 3000},
            "operations": ["retrieveEntity", "updateEntity"],
            "expiresAt": "2030-12-31T23:59:59Z",
            "timesSent": 3
        }));
        for (csf, selected) in [
            ("\"https://uri.etsi.org/ngsi-ld/registrationName\"==\"vehicles\"", true),
            ("\"https://uri.etsi.org/ngsi-ld/registrationName\"==\"buildings\"", false),
            ("\"http://purl.org/dc/terms/description\"~=\"telemetry\"", true),
            ("\"https://uri.etsi.org/ngsi-ld/management\"[timeout]==3000", true),
            ("\"https://uri.etsi.org/ngsi-ld/management\"[timeout]>3000", false),
            ("\"https://uri.etsi.org/ngsi-ld/operations\"==\"updateEntity\"", true),
            ("\"https://uri.etsi.org/ngsi-ld/operations\"==\"deleteEntity\"", false),
            ("\"https://uri.etsi.org/ngsi-ld/expiresAt\">2030-01-01T00:00:00Z", true),
            ("\"https://uri.etsi.org/ngsi-ld/expiresAt\"<2030-01-01T00:00:00Z", false),
            ("\"https://uri.etsi.org/ngsi-ld/status\"", false),
            ("\"https://uri.etsi.org/ngsi-ld/timesSent\"==3", false),
        ] {
            let mut sub = subscription();
            sub.compiled_csf = Some(parser::parse(csf).expect("a valid test fixture"));
            pretty_assertions::assert_eq!(notified_of_change(&sub, &declared), selected, "{csf}");
        }
    }
}
