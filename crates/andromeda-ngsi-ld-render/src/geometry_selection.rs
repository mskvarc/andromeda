//! The clause 4.5.16.1 top-level `geometry` field selection algorithm.
//!
//! Clause 4.5.16.1: "A parameter of the request (named `geometryProperty`) may
//! be used to indicate the name of the `GeoProperty` to be selected. If this
//! parameter is not present, then the default name of `location` shall be used.
//!
//! If the selected `GeoProperty` has multiple instances as described in clause
//! 4.5.5, either a `datasetId` shall be specified, in order to define which
//! instance of the value is to be selected, or a default attribute instance
//! exists, which is then selected, if no `datasetId` was specified.
//!
//! If an entity lacks the `GeoProperty` as specified or the value does not hold
//! a valid `GeoJSON` geometry object then the geometry shall be undefined and
//! returned with a value of null - which is syntactically valid `GeoJSON`."
//!
//! Clause 4.5.17.1 sends the simplified `GeoJSON` rendering back to this same
//! algorithm — "The selection of the geometry field is defined in clause
//! 4.5.16.1" — so the instance has to be found in whichever representation the
//! `properties` member was already rendered in: the array of instances clause
//! 4.5.5 gives the normalized form, or the `dataset` map keyed by `datasetId`
//! that clause 4.5.4 gives the simplified one.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::dataset::{DEFAULT_INSTANCE_TOKEN, DatasetSelector};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, compact_to_expanded};
use serde_json::{Map, Value};

/// The `GeoJSON` geometry types clause 4.7.1 admits as a `GeoProperty` value.
///
/// `GeometryCollection` is absent by design: ETSI GS CIM 009 v1.9.1 clause
/// 4.7.1 excludes it from `GeoProperty` geometries, so no stored `GeoProperty`
/// can be one.
const GEOMETRY_TYPES: &[&str] = &["Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon"];

/// The member naming a `GeoJSON` object's geometry type (IETF RFC 7946 section
/// 3.1), which is not the NGSI-LD attribute `type` of clause 4.5.2.2.
const GEOMETRY_TYPE_MEMBER: &str = "type";

/// The member holding a `GeoJSON` geometry's positions (IETF RFC 7946 section
/// 3.1). Every geometry clause 4.7.1 admits carries one.
const GEOMETRY_COORDINATES_MEMBER: &str = "coordinates";

/// Which `GeoProperty`, and which of its instances, a `GeoJSON` rendering takes
/// its geometry from (clause 4.5.16.1).
///
/// Both halves of the clause travel together because neither is usable alone:
/// naming the attribute without naming the instance is what leaves a
/// multi-instance `GeoProperty` with an arbitrary geometry, and naming the
/// instance without the attribute names nothing at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeometrySelection<'a> {
    /// The `geometryProperty` the request named; `location` when it named none.
    property: Option<&'a AttributeName>,
    /// The `datasetId` values the request named, empty when it named none.
    dataset_ids: &'a [DatasetSelector],
}

impl<'a> GeometrySelection<'a> {
    /// The selection a request expressed through `geometryProperty` and
    /// `datasetId`.
    #[must_use]
    pub const fn new(property: Option<&'a AttributeName>, dataset_ids: &'a [DatasetSelector]) -> GeometrySelection<'a> {
        GeometrySelection { property, dataset_ids }
    }
}

impl<'a> GeometrySelection<'a> {
    /// The name of the `GeoProperty` the geometry is taken from: the one the
    /// request named, `location` when it named none.
    pub(crate) fn property_name(&self) -> &'a str {
        self.property.map_or(compact_properties::LOCATION, AttributeName::as_str)
    }

    /// The `datasetId` values the request narrowed the instances to.
    pub(crate) const fn dataset_ids(&self) -> &'a [DatasetSelector] {
        self.dataset_ids
    }
}

impl GeometrySelection<'_> {
    /// The `GeoJSON` geometry of one rendered entity, or `None` when clause
    /// 4.5.16.1 selects none and the geometry is therefore null.
    pub(crate) fn geometry(&self, entity: &Map<String, Value>) -> Option<Value> {
        let located = self.geo_property(entity)?;
        let instance = self.instance(located)?;
        geojson_geometry(instance)
    }

    /// The member holding the selected `GeoProperty`.
    ///
    /// The attribute is read under the name the request gave it and then, if the
    /// core context declares one for that term, under its expanded IRI: a
    /// payload that was never compacted still carries the attribute, and which
    /// of the two spellings it is under cannot depend on whether the request
    /// named `location` or named a `GeoProperty` of its own.
    fn geo_property<'e>(&self, entity: &'e Map<String, Value>) -> Option<&'e Value> {
        let named = self.property_name();
        entity
            .get(named)
            .or_else(|| compact_to_expanded(named).and_then(|expanded| entity.get(expanded)))
    }

    /// The instance of the selected `GeoProperty` the geometry is taken from.
    ///
    /// Clause 4.5.16.1 admits exactly two outcomes for a multi-instance
    /// `GeoProperty`: the instance a `datasetId` named, or the default instance
    /// when no `datasetId` was specified. When neither exists the entity lacks
    /// "the `GeoProperty` as specified", which the same clause answers with a
    /// null geometry — never with an arbitrary instance, whose identity would
    /// then depend on the order the store happened to return the rows in.
    fn instance<'e>(&self, located: &'e Value) -> Option<&'e Value> {
        match located {
            // Clause 4.5.5, normalized: the instances are a JSON array.
            Value::Array(instances) => self.select_instance(instances),
            Value::Object(fields) => match fields.get(compact_properties::DATASET) {
                // Clause 4.5.4, simplified: one `dataset` member holding a map
                // keyed by each instance's `datasetId`, `@none` for the default.
                Some(Value::Object(datasets)) => self.select_dataset_member(datasets),
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) | None => self.select_sole_instance(located),
            },
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => self.select_sole_instance(located),
        }
    }

    /// Decide whether the single instance an attribute renders as is the one
    /// the request asked for.
    ///
    /// Clause 4.5.16.1 conditions its disambiguation on the `GeoProperty`
    /// having "multiple instances as described in clause 4.5.5", so a lone
    /// instance is the geometry whether or not it carries a `datasetId`: there
    /// is no other instance for a default to be chosen over. What the clause
    /// does not admit is answering a request that named a `datasetId` with an
    /// instance carrying a different one — the entity then lacks "the
    /// `GeoProperty` as specified", and the geometry is undefined, exactly as
    /// the array reading of the same attribute already decided.
    fn select_sole_instance<'e>(&self, instance: &'e Value) -> Option<&'e Value> {
        match instance_dataset_id(instance) {
            Some(dataset_id) if !self.dataset_ids.is_empty() => self.dataset_ids.iter().any(|selector| selects(selector, Some(dataset_id))).then_some(instance),
            Some(_) | None => Some(instance),
        }
    }

    /// Select one instance out of the array a multi-instance `GeoProperty`
    /// renders as.
    fn select_instance<'e>(&self, instances: &'e [Value]) -> Option<&'e Value> {
        if self.dataset_ids.is_empty() {
            return instances.iter().find(|instance| instance_dataset_id(instance).is_none());
        }
        self.dataset_ids
            .iter()
            .find_map(|selector| instances.iter().find(|instance| selects(selector, instance_dataset_id(instance))))
    }

    /// Select one member out of the clause 4.5.4 `dataset` map.
    fn select_dataset_member<'e>(&self, datasets: &'e Map<String, Value>) -> Option<&'e Value> {
        if self.dataset_ids.is_empty() {
            return datasets.get(DEFAULT_INSTANCE_TOKEN);
        }
        self.dataset_ids.iter().find_map(|selector| match selector {
            DatasetSelector::Default => datasets.get(DEFAULT_INSTANCE_TOKEN),
            DatasetSelector::Id(uri) => datasets.get(uri.as_str()),
        })
    }
}

/// The `datasetId` one rendered attribute instance carries, if any.
fn instance_dataset_id(instance: &Value) -> Option<&str> {
    instance.get(compact_properties::DATASET_ID).and_then(Value::as_str)
}

/// Whether one `datasetId` selector names the instance carrying `dataset_id`.
pub(crate) fn selects(selector: &DatasetSelector, dataset_id: Option<&str>) -> bool {
    match selector {
        DatasetSelector::Default => dataset_id.is_none(),
        DatasetSelector::Id(uri) => dataset_id == Some(uri.as_str()),
    }
}

/// Read the `GeoJSON` geometry out of one `GeoProperty` instance.
///
/// The normalized instance carries it under `value` (clause 4.5.2.2); the
/// concise and simplified renderings collapse to the geometry itself. Whichever
/// of the two the instance is, the value is answered only if it is one:
/// clause 4.5.16.1 makes the geometry undefined when "the value does not hold a
/// valid `GeoJSON` geometry object", and a top-level `geometry` member holding
/// something that is not one would leave the Feature outside IETF RFC 7946
/// altogether.
fn geojson_geometry(instance: &Value) -> Option<Value> {
    let candidate = instance.get(compact_properties::VALUE).unwrap_or(instance);
    // clone: the Feature owns the geometry it carries
    is_geojson_geometry(candidate).then(|| candidate.clone())
}

/// Whether a value is one of the `GeoJSON` geometry objects clause 4.7.1
/// admits.
///
/// IETF RFC 7946 section 3.1 gives a Geometry object a `type` naming one of the
/// seven geometry types and, for all but `GeometryCollection`, a `coordinates`
/// member; clause 4.7.1 excludes `GeometryCollection`, so both members are
/// required here.
fn is_geojson_geometry(value: &Value) -> bool {
    value
        .get(GEOMETRY_TYPE_MEMBER)
        .and_then(Value::as_str)
        .is_some_and(|kind| GEOMETRY_TYPES.contains(&kind))
        && value.get(GEOMETRY_COORDINATES_MEMBER).is_some()
}

#[cfg(test)]
mod tests {
    use crate::geometry_selection::GeometrySelection;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use serde_json::{Map, Value, json};

    fn entity(value: &Value) -> Map<String, Value> {
        value.as_object().expect("an entity object").clone()
    }

    fn selectors(values: &[&str]) -> Vec<DatasetSelector> {
        values.iter().map(|raw| raw.parse().expect("a valid dataset selector")).collect()
    }

    fn attribute(name: &str) -> AttributeName {
        name.parse().expect("a valid attribute name")
    }

    fn point(x: f64) -> Value {
        json!({"type": "Point", "coordinates": [x, 0.0]})
    }

    /// Clause 4.5.16.1: with no `geometryProperty` named, "the default name of
    /// `location` shall be used".
    #[test]
    fn the_default_geometry_property_is_location() {
        let entity = entity(&json!({"id": "urn:x", "location": {"type": "GeoProperty", "value": point(1.0)}}));
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&entity), Some(point(1.0)));
    }

    /// Clause 4.5.16.1: the request parameter "may be used to indicate the name
    /// of the `GeoProperty` to be selected".
    #[test]
    fn a_named_geometry_property_is_selected_instead_of_location() {
        let entity = entity(&json!({
            "id": "urn:x",
            "location": {"type": "GeoProperty", "value": point(1.0)},
            "observationSpace": {"type": "GeoProperty", "value": point(2.0)}
        }));
        let named = attribute("observationSpace");
        pretty_assertions::assert_eq!(GeometrySelection::new(Some(&named), &[]).geometry(&entity), Some(point(2.0)));
    }

    /// Clause 4.5.16.1: with multiple instances and no `datasetId` specified,
    /// "a default attribute instance exists, which is then selected".
    #[test]
    fn the_default_instance_is_selected_when_no_dataset_id_was_specified() {
        let entity = entity(&json!({
            "id": "urn:x",
            "location": [
                {"type": "GeoProperty", "value": point(1.0), "datasetId": "urn:ngsi-ld:Dataset:1"},
                {"type": "GeoProperty", "value": point(9.0)}
            ]
        }));
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&entity), Some(point(9.0)));
    }

    /// Clause 4.5.17.1 defers to clause 4.5.16.1, and clause 4.5.4 renders a
    /// multi-instance attribute as a `dataset` map whose default instance "is
    /// represented by the JSON-LD keyword `@none`", so the simplified rendering
    /// selects the same instance the normalized one does.
    #[test]
    fn the_default_instance_is_selected_out_of_the_simplified_dataset_map() {
        let entity = entity(&json!({
            "id": "urn:x",
            "location": {"dataset": {"urn:ngsi-ld:Dataset:1": point(1.0), "@none": point(9.0)}}
        }));
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&entity), Some(point(9.0)));
    }

    /// Clause 4.5.16.1: "either a `datasetId` shall be specified, in order to
    /// define which instance of the value is to be selected".
    #[test]
    fn a_named_dataset_id_selects_that_instance_in_both_representations() {
        let normalized = entity(&json!({
            "id": "urn:x",
            "location": [
                {"type": "GeoProperty", "value": point(1.0), "datasetId": "urn:ngsi-ld:Dataset:1"},
                {"type": "GeoProperty", "value": point(9.0)}
            ]
        }));
        let simplified = entity(&json!({
            "id": "urn:x",
            "location": {"dataset": {"urn:ngsi-ld:Dataset:1": point(1.0), "@none": point(9.0)}}
        }));
        let named = selectors(&["urn:ngsi-ld:Dataset:1"]);
        pretty_assertions::assert_eq!(GeometrySelection::new(None, &named).geometry(&normalized), Some(point(1.0)));
        pretty_assertions::assert_eq!(GeometrySelection::new(None, &named).geometry(&simplified), Some(point(1.0)));
    }

    /// Clause 4.5.16.1 admits two outcomes and no third: with several instances,
    /// none of them the default, and no `datasetId` specified, the entity lacks
    /// "the `GeoProperty` as specified" and "the geometry shall be undefined and
    /// returned with a value of null".
    #[test]
    fn no_default_instance_and_no_dataset_id_leaves_the_geometry_undefined() {
        let normalized = entity(&json!({
            "id": "urn:x",
            "location": [
                {"type": "GeoProperty", "value": point(1.0), "datasetId": "urn:ngsi-ld:Dataset:1"},
                {"type": "GeoProperty", "value": point(2.0), "datasetId": "urn:ngsi-ld:Dataset:2"}
            ]
        }));
        let simplified = entity(&json!({
            "id": "urn:x",
            "location": {"dataset": {"urn:ngsi-ld:Dataset:1": point(1.0), "urn:ngsi-ld:Dataset:2": point(2.0)}}
        }));
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&normalized), None);
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&simplified), None);
    }

    /// Clause 4.5.16.1, first half: "If an entity lacks the `GeoProperty` as
    /// specified [...] then the geometry shall be undefined."
    #[test]
    fn an_absent_geo_property_leaves_the_geometry_undefined() {
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&entity(&json!({"id": "urn:x"}))), None);
        let named = attribute("observationSpace");
        let elsewhere = entity(&json!({"id": "urn:x", "location": {"type": "GeoProperty", "value": point(1.0)}}));
        pretty_assertions::assert_eq!(GeometrySelection::new(Some(&named), &[]).geometry(&elsewhere), None);
    }

    /// Clause 4.5.16.1, second half: "or the value does not hold a valid
    /// `GeoJSON` geometry object then the geometry shall be undefined and
    /// returned with a value of null". The attribute is present and carries a
    /// value in every case here, so this is the branch the absent one never
    /// reaches — answering the raw value instead put a `geometry` member that is
    /// not a geometry into an IETF RFC 7946 Feature.
    #[test]
    fn a_value_that_is_not_a_geojson_geometry_leaves_the_geometry_undefined() {
        for value in [
            json!("not a geometry"),
            json!(42),
            json!({"type": "Point"}),
            json!({"coordinates": [1.0, 0.0]}),
            json!({"type": "Feature", "coordinates": [1.0, 0.0]}),
            json!({"type": "GeometryCollection", "geometries": []}),
            json!([1.0, 0.0]),
        ] {
            let normalized = entity(&json!({"id": "urn:x", "location": {"type": "GeoProperty", "value": value}}));
            pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&normalized), None, "normalized {value}");
            let simplified = entity(&json!({"id": "urn:x", "location": value}));
            pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&simplified), None, "simplified {value}");
        }
    }

    /// Clause 4.5.16.1 answers a request that named a `datasetId` with the
    /// instance carrying it, and with nothing when the entity carries no such
    /// instance. A lone instance is no exception: the array reading of the same
    /// attribute already refused it, and which of the two an attribute renders
    /// as is a clause 4.5.5 rendering detail rather than a selection rule.
    #[test]
    fn a_lone_instance_is_not_selected_by_a_dataset_id_it_does_not_carry() {
        let entity = entity(&json!({
            "id": "urn:x",
            "location": {"type": "GeoProperty", "value": point(1.0), "datasetId": "urn:ngsi-ld:Dataset:1"}
        }));
        let other = selectors(&["urn:ngsi-ld:Dataset:2"]);
        pretty_assertions::assert_eq!(GeometrySelection::new(None, &other).geometry(&entity), None);

        let named = selectors(&["urn:ngsi-ld:Dataset:1"]);
        pretty_assertions::assert_eq!(GeometrySelection::new(None, &named).geometry(&entity), Some(point(1.0)));

        // Clause 4.5.16.1 disambiguates only "if the selected GeoProperty has
        // multiple instances", so with one instance and no datasetId named
        // there is nothing to disambiguate.
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&entity), Some(point(1.0)));
    }

    /// The attribute is looked up the same way whichever name the request gave
    /// it: under the term, and then under the expanded IRI the core context
    /// declares for that term. `location` reaching an uncompacted payload while
    /// a named `GeoProperty` did not was one lookup rule for two arms of one
    /// decision.
    #[test]
    fn a_named_geo_property_is_read_under_its_expanded_iri_exactly_as_location_is() {
        let location = entity(&json!({
            "id": "urn:x",
            "https://uri.etsi.org/ngsi-ld/location": {"type": "GeoProperty", "value": point(1.0)}
        }));
        pretty_assertions::assert_eq!(GeometrySelection::default().geometry(&location), Some(point(1.0)));

        let observed = entity(&json!({
            "id": "urn:x",
            "https://uri.etsi.org/ngsi-ld/observationSpace": {"type": "GeoProperty", "value": point(2.0)}
        }));
        let named = attribute("observationSpace");
        pretty_assertions::assert_eq!(GeometrySelection::new(Some(&named), &[]).geometry(&observed), Some(point(2.0)));
    }
}
