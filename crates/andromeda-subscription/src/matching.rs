//! Subscription matching and entity-level filtering (geoQ, scopeQ, q).
//!
//! Pure domain logic for determining whether an entity change event matches
//! a compiled subscription. Used by the notification worker (event-driven
//! delivery) and the subscription cache (notification-event generation).

use crate::compiled::CompiledSubscription;
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_model::notification::ChangeOrigin;
use andromeda_ngsi_ld_query::{
    filter::eval::evaluate,
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
};
use chrono::Utc;
use tracing::debug;

/// Determine whether a compiled subscription matches a change event.
///
/// Checks the subscription's standing, the change's triggers and watched
/// attributes, entity selectors, and localOnly filtering (ETSI GS CIM 009
/// v1.9.1, clause 5.5.13).
pub fn matches_subscription(sub: &CompiledSubscription, event: &ChangeEvent) -> bool {
    if !sub.standing.notifies_at(Utc::now()) {
        debug!("Subscription {} skipped: not notified as it stands ({:?})", sub.id, sub.standing);
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

    // Table 5.2.12-1: the listed triggers "indicate what kind of changes shall
    // trigger a notification", and clause 5.8.1.4 makes the watched Attributes
    // "those that trigger a new notification when they are changed". Both only
    // decide whether this change is owed to the subscription; a change owed is
    // one notification however many of the triggers it raised.
    if !event.change.is_owed_to(&sub.triggers, sub.watched_attrs.as_ref()) {
        debug!(
            "Subscription {} skipped: change {:?} raises none of {:?} on watched attributes {:?}",
            sub.id,
            event.change.triggers(),
            sub.triggers,
            sub.watched_attrs
        );
        return false;
    }

    if !sub.selectors.is_empty() && !sub.selectors.iter().any(|selector| selector.selects(&event.entity_types, &event.entity_id)) {
        debug!(
            "Subscription {} skipped: no selector selects event {} typed {:?}",
            sub.id, event.entity_id, event.entity_types
        );
        return false;
    }

    debug!(
        "Subscription {} matched event {} (triggers={:?}, types={:?}, attrs={:?})",
        sub.id,
        event.entity_id,
        event.change.triggers(),
        event.entity_types,
        event.change.attributes()
    );
    true
}

/// Evaluate q (clause 4.9), geoQ (clause 4.10), scopeQ (clause 4.19) and csf
/// against an entity, the entities a `rel{…}` hop crosses into read from
/// `linked`.
///
/// Returns `true` if the entity passes all configured filters on the subscription.
/// Filters that are not configured (None) are skipped. On an entity
/// subscription `csf` is a second Query Language expression over the same
/// entity. The entity is a stored state or a received document, read through
/// the query language's own reading of an entity.
pub fn matches_entity_filters<'a, E, L>(sub: &CompiledSubscription, entity: E, linked: L) -> bool
where
    E: QueriedEntity<'a> + LocatedEntity<'a> + ScopedEntity<'a>,
    L: LinkedEntities<'a>,
{
    if let Some(ref expr) = sub.compiled_q
        && !evaluate(expr, entity, linked)
    {
        debug!("Subscription {} skipped: q filter did not match entity", sub.id);
        return false;
    }

    if !sub.compiled_geo_q.admits(|geo_q| geo_q.matches_entity(entity)) {
        debug!("Subscription {} skipped: geoQ filter did not admit entity", sub.id);
        return false;
    }

    if let Some(ref scope_q) = sub.compiled_scope_q
        && !scope_q.matches_entity(entity)
    {
        debug!("Subscription {} skipped: scopeQ filter did not match entity", sub.id);
        return false;
    }

    if let Some(ref csf_expr) = sub.compiled_csf
        && !evaluate(csf_expr, entity, linked)
    {
        debug!("Subscription {} skipped: csf filter did not match entity", sub.id);
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::{
        compiled::CompiledSubscription,
        error::SubscriptionError,
        geo_filter::{SubscriptionGeoFilter, compile_geo_q},
        matching::{matches_entity_filters, matches_subscription},
    };
    use andromeda_context_commit::{change_event::ChangeEvent, subscription_key::SubscriptionKey};
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::{ChangeOrigin, NotificationTrigger},
        subscription::{SubscriptionStatus, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        geo::{admission::GeoQueryAdmission, compile::compile_geo_query, input::GeoQueryInput, near_distance_band::NearDistanceBand},
        projection::compiled::Projection,
        queried_entity::linked_entities::NoLinkedEntities,
        term_expander::{TermExpander, TermExpansionFailure},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use futures::executor::block_on;
    use iri_rs::IriBuf;
    use std::{
        fmt::Display,
        future::{Future, ready},
        num::{NonZeroU32, NonZeroUsize},
        str::FromStr,
    };
    use url::Url;

    fn parse_fixture<T>(value: &str, description: &str) -> T
    where
        T: FromStr,
        T::Err: Display,
    {
        value.parse().unwrap_or_else(|error| panic!("invalid {description} fixture: {error}"))
    }

    /// A stored `geoQ`, read as the request model reads it.
    fn stored_geo_q(wire: serde_json::Value) -> GeoQueryInput {
        serde_json::from_value(wire).expect("a readable geoQ")
    }

    fn make_sub() -> CompiledSubscription {
        CompiledSubscription {
            id: parse_fixture("urn:ngsi-ld:Subscription:test", "subscription URI"),
            key: SubscriptionKey::try_from(1).expect("invalid subscription key fixture:"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("invalid endpoint fixture:"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: vec![NotificationTrigger::EntityCreated, NotificationTrigger::EntityUpdated],
            watched_attrs: None,
            selectors: Vec::new(),
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: None,
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: Projection::default(),
        }
    }

    fn change(lifecycle: EntityLifecycle, kind: AttributeChangeKind) -> EntityChange {
        EntityChange::new(
            lifecycle,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: parse_fixture("temperature", "attribute name"),
                    dataset_id: None,
                },
                kind,
            }],
        )
    }

    fn event_of(change: EntityChange) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(parse_fixture("urn:ngsi-ld:Entity:001", "entity URI"))
            .entity_types(vec![parse_fixture("Building", "entity type")])
            .change(change)
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    fn make_event() -> ChangeEvent {
        event_of(change(EntityLifecycle::Created, AttributeChangeKind::Created))
    }

    fn at(unix_seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range")
    }

    /// Clause 5.2.12: a subscription has no initial notification, so a change
    /// that occurred before the subscription was created never notifies it. The
    /// notification worker can still see such a change after the subscription has
    /// entered the cache, which is when this guard has to hold.
    #[test]
    fn a_change_that_occurred_before_the_subscription_was_created_is_not_matched() {
        let mut event = make_event();
        event.occurred_at = at(1_000);
        let mut sub = make_sub();

        sub.created_at = at(2_000);
        assert!(
            !matches_subscription(&sub, &event),
            "a change before the subscription's creation must not match"
        );

        sub.created_at = at(1_000);
        assert!(
            matches_subscription(&sub, &event),
            "a change at the subscription's creation instant still matches"
        );

        sub.created_at = at(500);
        assert!(matches_subscription(&sub, &event), "a change after the subscription's creation matches");
    }

    /// Resolves every term to the NGSI-LD `location` `GeoProperty`, which is what
    /// the core context does for the default `geoproperty`.
    struct LocationExpander;

    impl TermExpander for LocationExpander {
        fn expand_term(&self, _term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(IriBuf::new(LOCATION.to_owned()).map_err(|_| QueryError::TermExpansion(TermExpansionFailure::NotAnIri)))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(IriBuf::new(format!("https://example.com/{term}")).map_err(|_| QueryError::TermExpansion(TermExpansionFailure::NotAnIri)))
        }
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// A stored entity whose `location` is the point given.
    fn located(longitude: f64, latitude: f64) -> EntityState {
        state(&serde_json::json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": ["https://example.com/Building"],
            LOCATION: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
                }]
            }]
        }]))
    }

    /// Decode a stored document into a state.
    fn state(document: &serde_json::Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes")
    }

    #[test]
    fn a_subscription_geo_filter_is_the_same_compiled_type_a_query_uses() -> Result<(), SubscriptionError> {
        // Clause 4.10 is one relation, so a subscription's stored `geoQ` and a
        // request's `georel`/`geometry`/`coordinates` must compile to the same
        // value, evaluated in memory by the one evaluator.
        let from_subscription = compile_geo_q(&stored_geo_q(serde_json::json!({
            "georel": "near;maxDistance==1000",
            "geometry": "Point",
            "coordinates": [13.4, 52.5],
            "geoproperty": LOCATION
        })))
        .expect("the subscription geoQ fixture did not compile");

        let request_geo = GeoQueryInput::from_wire(
            "near;maxDistance==1000",
            "Point",
            serde_json::from_value(serde_json::json!([13.4, 52.5])).expect("coordinates"),
            Some(parse_fixture("location", "geoproperty")),
        )
        .expect("a valid test fixture");
        let limits = GeoQueryAdmission::new(
            GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MAX),
                values: JsonValueLimit::new(NonZeroUsize::MAX),
            },
            NearDistanceBand::Refused,
        );
        let from_request =
            block_on(compile_geo_query(Some(&request_geo), &LocationExpander, limits))?.expect("the complete request geo fixture did not compile");

        pretty_assertions::assert_eq!(from_subscription, from_request);
        Ok(())
    }

    #[test]
    fn a_subscription_geo_filter_accepts_only_entities_inside_the_relation() {
        let geo_query = compile_geo_q(&stored_geo_q(serde_json::json!({
            "georel": "near;maxDistance==1000",
            "geometry": "Point",
            "coordinates": [13.4, 52.5],
            "geoproperty": LOCATION
        })))
        .expect("the subscription geoQ fixture did not compile");

        assert!(geo_query.matches_entity(&located(13.4, 52.5)));
        assert!(!geo_query.matches_entity(&located(13.4, 53.5)));
        assert!(!geo_query.matches_entity(&state(
            &serde_json::json!([{"@id": "urn:ngsi-ld:Building:002", "@type": ["https://example.com/Building"]}])
        )));
    }

    /// Clause 5.8.6: every changed entity is included only when no geoquery is
    /// defined. A stored `geoQ` that no longer compiles is defined, so it
    /// admits no entity instead of every one.
    #[test]
    fn a_stored_geo_q_that_no_longer_compiles_admits_no_entity() {
        let mut sub = make_sub();
        assert!(
            matches_entity_filters(&sub, &located(13.4, 52.5), NoLinkedEntities),
            "no geoQ admits every entity"
        );

        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(
            Some(&stored_geo_q(
                serde_json::json!({"georel": "near;maxDistance==1000", "geometry": "Point", "coordinates": "[13.4, "}),
            )),
            "urn:ngsi-ld:Subscription:test",
        );
        assert!(
            !matches_entity_filters(&sub, &located(13.4, 52.5), NoLinkedEntities),
            "an unusable geoQ admits nothing"
        );

        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(
            Some(&stored_geo_q(
                serde_json::json!({"georel": "near;maxDistance==1000", "geometry": "Point", "coordinates": [13.4, 52.5]}),
            )),
            "urn:ngsi-ld:Subscription:test",
        );
        assert!(
            matches_entity_filters(&sub, &located(13.4, 52.5), NoLinkedEntities),
            "a compiled geoQ admits what it matches"
        );
        assert!(!matches_entity_filters(&sub, &located(13.4, 53.5), NoLinkedEntities), "and nothing else");
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
        sub.standing.status = SubscriptionStatus::Paused;
        let event = make_event();
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn subscription_skips_wrong_trigger() {
        let sub = make_sub();
        let event = event_of(change(EntityLifecycle::Deleted, AttributeChangeKind::Deleted));
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
