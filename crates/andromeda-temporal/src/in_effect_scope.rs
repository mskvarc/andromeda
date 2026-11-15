//! The Scope a split temporal query's scope filter reads in force when its
//! window opens, merged over every part of an entity (clauses 4.18 and
//! 5.7.4.4).
//!
//! Clause 4.18 makes the Scope valid from the time it has been set, so the
//! Scope in force when a window opens is the latest Scope event placed before
//! the start (Annex C clause C.5.16). Under clause 5.7.4.4 one entity may be
//! held in several parts, so that event is the latest across all of them. An
//! event is placed at its instant on the window's Temporal Property and a
//! removal carrying none there at the instant it was deleted (clause 4.5.7).
//! A removal at the winning placement leaves no Scope in force.
//!
//! The filters that read attribute instances (`q` and the geo-query) read the
//! instances inside the window alone (clause 5.7.4.4 steps S2 and S3), so this
//! entity names no attribute instance.

use crate::reified_scope::reified_paths;
use andromeda_ngsi_ld_entity::document::{attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    scope::path::ScopePath,
    time::date_time::NgsiLdDateTime,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::temporal::window::TemporalWindow;
use andromeda_ngsi_ld_query_evaluation::queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use std::{borrow::Cow, iter};

/// The Scope in force when a window opens, as the filters of a split query
/// read the entity.
#[derive(Debug)]
pub(crate) struct InEffectScope<'a> {
    /// The paths the Scope in force names.
    scopes: Vec<ScopePath>,
    /// The entity's types, as the first part naming any names them: a part
    /// this broker read from its history names none.
    types: &'a [EntityTypeName],
}

/// The latest Scope events placed before the start among the parts read so
/// far.
#[derive(Debug)]
struct Latest<'a> {
    /// The placement they share.
    placed: DateTime<Utc>,
    /// Every event at that placement.
    instances: Vec<&'a InstanceDocument>,
}

impl<'a> Latest<'a> {
    /// The latest of one event placed at `placed`.
    fn of(placed: DateTime<Utc>, instance: &'a InstanceDocument) -> Latest<'a> {
        Latest {
            placed,
            instances: vec![instance],
        }
    }

    /// Take `instance`, placed at `placed`, into account.
    fn offer(&mut self, placed: DateTime<Utc>, instance: &'a InstanceDocument) {
        if placed > self.placed {
            *self = Latest::of(placed, instance);
        } else if placed == self.placed {
            self.instances.push(instance);
        }
    }
}

impl<'a> InEffectScope<'a> {
    /// The Scope `parts` hold in force when `window` opens, merged; `None`
    /// when the window has no Scope in effect before it.
    pub(crate) fn merged(parts: &[&'a TemporalDocument], window: TemporalWindow) -> Option<InEffectScope<'a>> {
        let start = window.in_effect_start()?;
        let mut scope: Option<Latest<'a>> = None;
        for part in parts {
            let instances = part
                .entity()
                .attributes()
                .get(expanded_properties::SCOPE.as_str())
                .into_iter()
                .flat_map(AttributeMember::instances);
            for instance in instances {
                let Some(placed) = placement(instance, window.timeproperty).filter(|placed| *placed < start) else {
                    continue;
                };
                match &mut scope {
                    Some(latest) => latest.offer(placed, instance),
                    None => scope = Some(Latest::of(placed, instance)),
                }
            }
        }
        Some(InEffectScope {
            scopes: scope.map_or_default(|latest| reified_paths(latest.instances)),
            types: parts
                .iter()
                .map(|part| part.entity().types())
                .find(|types| !types.is_empty())
                .unwrap_or_default(),
        })
    }
}

/// Where `instance` is placed on `timeproperty`: at its instant there, or, for
/// a removal carrying none, at the instant it was deleted (clause 4.5.7).
fn placement(instance: &InstanceDocument, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
    instance.instant_on(timeproperty).or_else(|| instance.deleted_at().map(NgsiLdDateTime::get))
}

impl<'a> QueriedEntity<'a> for &'a InEffectScope<'a> {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, _attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        iter::empty()
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        self.types.iter()
    }
}

impl<'a> LocatedEntity<'a> for &'a InEffectScope<'a> {
    fn geo_property_geometries(self, _attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        iter::empty()
    }
}

impl<'a> ScopedEntity<'a> for &'a InEffectScope<'a> {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        self.scopes.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{answered_fixture::answered, in_effect_scope::InEffectScope};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
        split_filter::SplitFilter,
        temporal::window::{TemporalWindow, TimeInterval},
    };
    use andromeda_ngsi_ld_query_evaluation::{queried_entity::linked_entities::NoLinkedEntities, split_filter::leaf_truths};
    use serde_json::{Value, json};

    /// The window from 12:00 to 13:00 on `observedAt`.
    fn noon_to_one() -> TemporalWindow {
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Between {
                from: at("2020-08-01T12:00:00Z"),
                to: at("2020-08-01T13:00:00Z"),
            })
            .build()
    }

    /// A part of Vehicle 1 holding the Scope events `instances`.
    async fn scoped(instances: &Value) -> TemporalDocument {
        answered(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "scope": instances})).await
    }

    /// A Scope event naming `path`, observed at `at`.
    fn scope_event(path: &str, at: &str) -> Value {
        json!({"type": "Property", "value": path, "observedAt": at})
    }

    /// The query for the Scope `/Madrid`.
    fn madrid() -> EntitySelection {
        EntitySelection::builder()
            .scope_q(Some(ScopeQuery::parse("/Madrid").expect("a valid scopeQ")))
            .build()
    }

    /// Whether the filters of `selection` hold over the Scope in force of
    /// `parts`.
    fn holds(selection: &EntitySelection, parts: &[&TemporalDocument]) -> bool {
        let filter = SplitFilter::of(selection);
        let merged = InEffectScope::merged(parts, noon_to_one()).expect("a window with a start");
        filter.holds(&leaf_truths(&filter, &merged, NoLinkedEntities))
    }

    /// Clause 4.18: the Scope in force at the start is the latest Scope event
    /// before it across the parts, an event inside the window none of it.
    #[tokio::test]
    async fn the_latest_scope_before_the_start_is_in_force() {
        let local = scoped(&json!([scope_event("/Madrid", "2020-08-01T11:00:00Z")])).await;
        let remote = scoped(&json!([scope_event("/Spain", "2020-08-01T11:30:00Z")])).await;
        let later = scoped(&json!([scope_event("/Madrid", "2020-08-01T12:30:00Z")])).await;
        assert!(holds(&madrid(), &[&local]));
        assert!(!holds(&madrid(), &[&local, &remote, &later]));
    }

    /// Clause 4.5.7: a removal places the Scope at the instant it was deleted
    /// and leaves no Scope in force, so a newer removal in another part ends
    /// the older event.
    #[tokio::test]
    async fn a_newer_removal_ends_the_scope_in_force() {
        let local = scoped(&json!([scope_event("/Madrid", "2020-08-01T11:00:00Z")])).await;
        let remote = scoped(&json!([{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2020-08-01T11:30:00Z"}])).await;
        assert!(holds(&madrid(), &[&local]));
        assert!(!holds(&madrid(), &[&local, &remote]));
    }

    /// Clause 4.11 opens the window inclusively: a Scope event at the start,
    /// or later, lies inside the window and is never the one in force.
    #[tokio::test]
    async fn a_scope_event_at_or_after_the_start_is_not_in_force() {
        let at_start = scoped(&json!([scope_event("/Madrid", "2020-08-01T12:00:00Z")])).await;
        let later = scoped(&json!([scope_event("/Madrid", "2020-08-01T12:30:00Z")])).await;
        assert!(!holds(&madrid(), &[&at_start, &later]));
    }

    /// Clause 5.7.4.4 steps S2 and S3: an attribute instance placed before the
    /// start is read by neither `q` nor the geo-query, however recent it is.
    #[tokio::test]
    async fn an_attribute_instance_before_the_start_is_read_by_no_filter() {
        let part = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T11:00:00Z"}],
            "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}, "observedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        let above = EntitySelection::builder()
            .q(Some(CompiledFilter::new(
                "\"https://example.com/speed\">50".parse().expect("a valid q"),
                LanguageMatching::TagEquality,
            )))
            .build();
        let within_berlin = EntitySelection::builder()
            .geo(Some(GeoQuery {
                georel: GeoRel::Within,
                geometry: serde_json::from_value::<Geometry>(json!({
                    "type": "Polygon",
                    "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
                }))
                .expect("a polygon"),
                geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("a valid attribute name"),
            }))
            .build();
        assert!(!holds(&above, &[&part]));
        assert!(!holds(&within_berlin, &[&part]));
    }

    /// A window with no start holds every instance before its end already, so
    /// no Scope is in force before it.
    #[tokio::test]
    async fn a_window_without_a_start_merges_nothing() {
        let part = scoped(&json!([scope_event("/Madrid", "2020-08-01T11:00:00Z")])).await;
        let before = TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Before(
                "2020-08-01T13:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get(),
            ))
            .build();
        assert!(InEffectScope::merged(&[&part], before).is_none());
    }

    /// Clause 5.2.21: the window's axis places the Scope events, so on
    /// `modifiedAt` the latest modification wins whatever was observed.
    #[tokio::test]
    async fn the_window_axis_places_the_scope_events() {
        let event = |path: &str, observed: &str, modified: &str| json!([{"type": "Property", "value": path, "observedAt": observed, "modifiedAt": modified}]);
        let older = scoped(&event("/Madrid", "2020-08-01T11:50:00Z", "2020-08-01T11:00:00Z")).await;
        let newer = scoped(&event("/Spain", "2020-08-01T11:10:00Z", "2020-08-01T11:30:00Z")).await;
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let on = |timeproperty| {
            TemporalWindow::builder()
                .timeproperty(timeproperty)
                .interval(TimeInterval::After(at("2020-08-01T12:00:00Z")))
                .build()
        };
        let selection = madrid();
        let filter = SplitFilter::of(&selection);
        let observed = InEffectScope::merged(&[&older, &newer], on(TimeProp::Observed)).expect("a window with a start");
        assert!(filter.holds(&leaf_truths(&filter, &observed, NoLinkedEntities)));
        let modified = InEffectScope::merged(&[&older, &newer], on(TimeProp::Modified)).expect("a window with a start");
        assert!(!filter.holds(&leaf_truths(&filter, &modified, NoLinkedEntities)));
    }
}
