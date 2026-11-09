//! The instances a split temporal query's filters read in effect when its
//! window opens, merged over every part of an entity (clauses 4.11, 4.18 and
//! 5.7.4.4).
//!
//! Clause 4.11 counts among the instances valid within a window the one each
//! stream held when it opens (Annex C clause C.5.5): the latest of the stream
//! placed before the start. Under clause 5.7.4.4 one stream of a split entity
//! may be held in several parts, so that instance is the latest across all of
//! them. Per attribute and `datasetId` every part's candidates are placed, an
//! instance at its instant on the window's Temporal Property and a deletion
//! carrying none there at the instant it was deleted (clause 4.5.7), and the
//! latest placement wins. A deletion at the winning placement holds no value,
//! so the stream holds nothing in effect; otherwise every instance at it is
//! read, as clause 4.9 matches "any Value of such instances". The Scope in
//! force at the start is the latest Scope event before it across the parts
//! (clause 4.18), read as the paths it names.

use crate::reified_scope::reified_paths;
use andromeda_ngsi_ld_entity::document::{instance_document::InstanceDocument, written_value::ValueSlot};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    scope::path::ScopePath,
    time::date_time::NgsiLdDateTime,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    temporal::window::TemporalWindow,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use std::{
    borrow::Cow,
    collections::{HashMap, hash_map::Entry},
};

/// One stream of an attribute: the attribute and the `datasetId` it is
/// written under, `None` for the default instance (clause 4.5.5).
type StreamOf<'a> = (&'a AttributeName, Option<&'a Identifier>);

/// The entity the in-effect instances of every part of it make up, as the
/// filters of a split query read it.
#[derive(Debug)]
pub(crate) struct InEffectEntity<'a> {
    /// The instances each attribute holds in effect, of every stream that
    /// holds a value.
    attributes: HashMap<&'a AttributeName, Vec<&'a InstanceDocument>>,
    /// The paths the Scope in force names.
    scopes: Vec<ScopePath>,
    /// The entity's types, as the first part naming any names them: a part
    /// this broker read from its history names none.
    types: &'a [EntityTypeName],
}

/// The latest instances of one stream, or of the Scope, placed before the
/// start among the parts read so far.
#[derive(Debug)]
struct Latest<'a> {
    /// The placement they share.
    placed: DateTime<Utc>,
    /// Every instance at that placement.
    instances: Vec<&'a InstanceDocument>,
}

impl<'a> Latest<'a> {
    /// The latest of one instance placed at `placed`.
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

    /// The instances a value is read from: none when a deletion is among
    /// them, since it ends the stream at that placement.
    fn holding_a_value(self) -> Option<Vec<&'a InstanceDocument>> {
        (!self.instances.iter().any(|instance| records_deletion(instance))).then_some(self.instances)
    }
}

impl<'a> InEffectEntity<'a> {
    /// The instances `parts` hold in effect when `window` opens, merged; `None`
    /// when the window has no instance in effect before it.
    pub(crate) fn merged(parts: &[&'a TemporalDocument], window: TemporalWindow) -> Option<InEffectEntity<'a>> {
        let start = window.in_effect_start()?;
        let mut streams: HashMap<StreamOf<'a>, Latest<'a>> = HashMap::new();
        let mut scope: Option<Latest<'a>> = None;
        for part in parts {
            for (attribute, member) in part.entity().attributes() {
                let is_scope = attribute.as_str() == expanded_properties::SCOPE.as_str();
                for instance in member.instances() {
                    let Some(placed) = placement(instance, window.timeproperty).filter(|placed| *placed < start) else {
                        continue;
                    };
                    if is_scope {
                        match &mut scope {
                            Some(latest) => latest.offer(placed, instance),
                            None => scope = Some(Latest::of(placed, instance)),
                        }
                        continue;
                    }
                    match streams.entry((attribute, instance.dataset_id())) {
                        Entry::Occupied(mut latest) => latest.get_mut().offer(placed, instance),
                        Entry::Vacant(vacant) => {
                            vacant.insert(Latest::of(placed, instance));
                        }
                    }
                }
            }
        }
        let mut attributes: HashMap<&'a AttributeName, Vec<&'a InstanceDocument>> = HashMap::new();
        for ((attribute, _), latest) in streams {
            if let Some(instances) = latest.holding_a_value() {
                attributes.entry(attribute).or_default().extend(instances);
            }
        }
        Some(InEffectEntity {
            attributes,
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
/// a deletion carrying none, at the instant it was deleted (clause 4.5.7).
pub(crate) fn placement(instance: &InstanceDocument, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
    instance.instant_on(timeproperty).or_else(|| instance.deleted_at().map(NgsiLdDateTime::get))
}

/// Whether `instance` records a deletion (clause 4.5.7), which holds no value.
const fn records_deletion(instance: &InstanceDocument) -> bool {
    instance.deleted_at().is_some() || matches!(instance.value(), ValueSlot::Remove(_))
}

impl<'a> QueriedEntity<'a> for &'a InEffectEntity<'a> {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        self.attributes.get(attribute).into_iter().flatten().copied()
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        self.types.iter()
    }
}

impl<'a> LocatedEntity<'a> for &'a InEffectEntity<'a> {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.attribute_instances(attribute)
            .filter_map(|instance| instance.geometry().map(Cow::Borrowed))
    }
}

impl<'a> ScopedEntity<'a> for &'a InEffectEntity<'a> {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        self.scopes.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{answered_fixture::answered, in_effect_entity::InEffectEntity};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        geo::query::GeoQuery,
        queried_entity::linked_entities::NoLinkedEntities,
        scope::query::ScopeQuery,
        split_filter::SplitFilter,
        temporal::window::{TemporalWindow, TimeInterval},
    };
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

    /// `georel=within` a box around `(13.4, 52.5)` on `location`.
    fn within_berlin() -> EntitySelection {
        EntitySelection::builder()
            .geo(Some(GeoQuery {
                georel: GeoRel::Within,
                geometry: serde_json::from_value::<Geometry>(json!({
                    "type": "Polygon",
                    "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
                }))
                .expect("a polygon"),
                geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("a valid attribute name"),
            }))
            .build()
    }

    /// A part of Vehicle 1 holding `location` instances.
    async fn located(instances: &Value) -> TemporalDocument {
        answered(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "location": instances})).await
    }

    /// A `location` instance at `[longitude, latitude]` observed at `at`.
    fn point(longitude: f64, latitude: f64, at: &str) -> Value {
        json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, latitude]}, "observedAt": at})
    }

    /// Whether the filters of `selection` hold over the in-effect entity of
    /// `parts`.
    fn holds(selection: &EntitySelection, parts: &[&TemporalDocument]) -> bool {
        let filter = SplitFilter::of(selection);
        let merged = InEffectEntity::merged(parts, noon_to_one()).expect("a window with a start");
        filter.holds(&filter.truths_of(&merged, NoLinkedEntities))
    }

    /// Clause 4.11 with clause 5.7.4.4: the instance in effect at 12:00 is the
    /// latest across both parts, the remote one at 11:50 outside the polygon,
    /// so the local 11:00 instance inside it no longer counts.
    #[tokio::test]
    async fn an_in_effect_instance_superseded_by_a_newer_part_does_not_hold() {
        let local = located(&json!([point(13.4, 52.5, "2020-08-01T11:00:00Z")])).await;
        let remote = located(&json!([point(0.0, 0.0, "2020-08-01T11:50:00Z")])).await;
        assert!(!holds(&within_berlin(), &[&remote, &local]));
        assert!(holds(&within_berlin(), &[&local]));
    }

    /// The instance in effect may be held by a remote part alone.
    #[tokio::test]
    async fn an_in_effect_instance_from_a_remote_part_holds() {
        let remote = located(&json!([point(13.4, 52.5, "2020-08-01T11:00:00Z")])).await;
        assert!(holds(&within_berlin(), &[&remote]));
    }

    /// Clause 4.5.7: a deletion places the stream at the instant it was
    /// deleted and holds no value, so a newer deletion in another part ends
    /// the older instance.
    #[tokio::test]
    async fn a_newer_deletion_cancels_an_earlier_in_effect_instance() {
        let local = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        let remote = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2020-08-01T11:30:00Z"}]
        }))
        .await;
        let above = EntitySelection::builder()
            .q_expr(Some("\"https://example.com/speed\">50".parse().expect("a valid q")))
            .build();
        assert!(holds(&above, &[&local]));
        assert!(!holds(&above, &[&local, &remote]));
    }

    /// Clause 4.5.5: each `datasetId` is a stream of its own, so a newer
    /// instance of another dataset leaves the older one in effect.
    #[tokio::test]
    async fn each_dataset_keeps_its_own_in_effect_instance() {
        let mut inside = point(13.4, 52.5, "2020-08-01T11:00:00Z");
        inside["datasetId"] = json!("urn:ngsi-ld:dataset:d1");
        let mut outside = point(0.0, 0.0, "2020-08-01T11:40:00Z");
        outside["datasetId"] = json!("urn:ngsi-ld:dataset:d2");
        let local = located(&json!([inside])).await;
        let remote = located(&json!([outside])).await;
        assert!(holds(&within_berlin(), &[&local, &remote]));
    }

    /// Clause 4.18: the Scope in force at the start is the latest Scope event
    /// before it across the parts, an event inside the window none of it.
    #[tokio::test]
    async fn the_latest_scope_before_the_start_is_in_force() {
        let scoped =
            |path: &str, at: &str| json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "scope": [{"type": "Property", "value": path, "observedAt": at}]});
        let local = answered(&scoped("/Madrid", "2020-08-01T11:00:00Z")).await;
        let remote = answered(&scoped("/Spain", "2020-08-01T11:30:00Z")).await;
        let later = answered(&scoped("/Madrid", "2020-08-01T12:30:00Z")).await;
        let madrid = EntitySelection::builder()
            .scope_q(Some(ScopeQuery::parse("/Madrid").expect("a valid scopeQ")))
            .build();
        assert!(holds(&madrid, &[&local]));
        assert!(!holds(&madrid, &[&local, &remote, &later]));
    }

    /// Clause 4.9 matches "any Value of such instances": every instance at the
    /// latest placement is in effect, a repeated write at one instant included.
    #[tokio::test]
    async fn every_instance_at_the_latest_placement_is_in_effect() {
        let local = located(&json!([point(0.0, 0.0, "2020-08-01T11:00:00Z")])).await;
        let remote = located(&json!([point(13.4, 52.5, "2020-08-01T11:00:00Z")])).await;
        assert!(holds(&within_berlin(), &[&local, &remote]));
    }

    /// Clause 4.11 opens the window inclusively: an instance at the start, or
    /// later, lies inside the window and is never the one in effect.
    #[tokio::test]
    async fn an_instance_at_or_after_the_start_is_not_in_effect() {
        let at_start = located(&json!([point(13.4, 52.5, "2020-08-01T12:00:00Z")])).await;
        let later = located(&json!([point(13.4, 52.5, "2020-08-01T12:30:00Z")])).await;
        assert!(!holds(&within_berlin(), &[&at_start, &later]));
    }

    /// A window with no start holds every instance before its end already, so
    /// no instance is in effect before it.
    #[tokio::test]
    async fn a_window_without_a_start_merges_nothing() {
        let part = located(&json!([point(13.4, 52.5, "2020-08-01T11:00:00Z")])).await;
        let before = TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Before(
                "2020-08-01T13:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get(),
            ))
            .build();
        assert!(InEffectEntity::merged(&[&part], before).is_none());
    }

    /// Clause 5.2.21: each axis places on its own Temporal Property, so on
    /// `modifiedAt` the latest modification wins whatever was observed.
    #[tokio::test]
    async fn the_window_axis_places_the_instances() {
        let older = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T11:50:00Z", "modifiedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        let newer = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T11:10:00Z", "modifiedAt": "2020-08-01T11:30:00Z"}]
        }))
        .await;
        let above = EntitySelection::builder()
            .q_expr(Some("\"https://example.com/speed\">50".parse().expect("a valid q")))
            .build();
        let filter = SplitFilter::of(&above);
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let on = |timeproperty| {
            TemporalWindow::builder()
                .timeproperty(timeproperty)
                .interval(TimeInterval::After(at("2020-08-01T12:00:00Z")))
                .build()
        };
        let observed = InEffectEntity::merged(&[&older, &newer], on(TimeProp::Observed)).expect("a window with a start");
        assert!(filter.holds(&filter.truths_of(&observed, NoLinkedEntities)));
        let modified = InEffectEntity::merged(&[&older, &newer], on(TimeProp::Modified)).expect("a window with a start");
        assert!(!filter.holds(&filter.truths_of(&modified, NoLinkedEntities)));
    }
}
