//! Decoding `GeoProperty` values out of an expanded payload (ETSI GS CIM 009 clause 4.7).

use crate::expanded::{
    attribute_expiry::instance_expires_at,
    dataset_id::instance_has_dataset_id,
    geo_node::expanded_geo_node_to_geometry,
    node::expanded_object_ref,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Extract the entity's `location` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_location_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::LOCATION.as_str())
}

/// The clause 4.22 `expiresAt` of the entity's default `location` instance --
/// the one [`extract_location_geometry`] decodes -- or `None` when it carries
/// none.
///
/// The default instance is hoisted beside its expiry so a geo query can read
/// both from the search row; clause 4.22 makes that instance invalid once the
/// instant has passed, whatever the entity's other `location` instances carry,
/// so the geometry and the expiry must come from the same instance. An entity
/// without a `location` yields `None` as well; the geometry column being NULL
/// already excludes it.
#[must_use]
pub fn extract_location_expiry(expanded: &Value) -> Option<DateTime<Utc>> {
    let instances = geo_property_instances(expanded, properties::LOCATION.as_str())?;
    instance_expires_at(default_instance(instances)?)
}

/// Extract the entity's `observationSpace` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_observation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OBSERVATION_SPACE.as_str())
}

/// Extract the entity's `operationSpace` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_operation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OPERATION_SPACE.as_str())
}

/// Decode the default instance of the `GeoProperty` named by `iri` into a
/// [`Geometry`], accepting both the `GeoJSON` literal form and the fully expanded
/// node form.
///
/// One geometry per entity, chosen by [`default_instance`] rather than by array
/// order. This is what the spatial index is built from: an R-tree partition
/// holds one bounding rectangle per entity, so a multi-instance `GeoProperty` has
/// to be reduced to a single geometry before it can be indexed, and clause
/// 4.5.16.1's default instance is the only defensible choice of which.
///
/// Matching is a different question and does not use this reader: clause
/// 5.7.2.4 counts an entity as matching when *any* instance meets the
/// restriction, for which [`geo_property_instance_geometries`] yields all of
/// them. An index built from this reader therefore answers for the default
/// instance alone, and its consumers treat it so: the entity query narrows only
/// the store's default-instance arm with it and still evaluates every other
/// instance, and the notification path evaluates every instance through the
/// residual filter.
#[must_use]
pub fn extract_geo_property_geometry(expanded: &Value, iri: &str) -> Option<Geometry> {
    let instances = geo_property_instances(expanded, iri)?;
    geometry_of_instance(default_instance(instances)?)
}

/// Decode every instance of the `GeoProperty` named by `iri`, in payload order.
///
/// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one of
/// them meeting a geo restriction is enough, so a matcher reads all of them
/// rather than the default one. Instances that carry no decodable geometry are
/// skipped, exactly as the SQL rendering skips `current_attribute_values` rows whose
/// `value_geometry` is null.
pub fn geo_property_instance_geometries<'a>(expanded: &'a Value, iri: &str) -> impl Iterator<Item = Geometry> + 'a {
    geo_property_instances(expanded, iri)
        .unwrap_or_default()
        .iter()
        .filter_map(geometry_of_instance)
}

/// The instance array of one expanded attribute, if the entity conveys it.
fn geo_property_instances<'a>(expanded: &'a Value, iri: &str) -> Option<&'a [Value]> {
    expanded_object_ref(expanded)?.get(iri)?.as_array().map(Vec::as_slice)
}

/// Decode one attribute instance's value into a [`Geometry`].
///
/// Reads the "{@type, ...coordinates}" object that lives at
/// `<instance>.hasValue[0]`, ignoring whether the value was inlined as a
/// literal `@value` or expanded as a nested JSON-LD `GeoJSON` node.
fn geometry_of_instance(instance: &Value) -> Option<Geometry> {
    let geo_node = instance
        .get(properties::VALUE.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())?;
    if let Some(literal) = geo_node.get("@value") {
        return literal_geojson_to_geometry(literal);
    }
    expanded_geo_node_to_geometry(geo_node)
}

/// Select the default instance of a multi-attribute (clause 4.5.16.1): the one
/// carrying no `datasetId`, falling back to the first when all are tagged.
///
/// Selection is deliberate rather than an accident of `first()` versus
/// `last()`, so two readers of the same payload cannot disagree.
fn default_instance(instances: &[Value]) -> Option<&Value> {
    instances
        .iter()
        .find(|instance| !instance_has_dataset_id(instance))
        .or_else(|| instances.first())
}

/// Decode a literal-GeoJSON `@value` (string or already-compact object) into a [`Geometry`].
fn literal_geojson_to_geometry(literal: &Value) -> Option<Geometry> {
    match literal {
        Value::String(s) => {
            let value: Value = serde_json::from_str(s).ok()?;
            Geometry::from_geojson_value(&value).ok()
        }
        Value::Object(_) => Geometry::from_geojson_value(literal).ok(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::geo_property::{extract_geo_property_geometry, extract_location_expiry, extract_location_geometry, geo_property_instance_geometries};
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// Wrap a `location` `GeoProperty` carrying `value` in one instance, without
    /// a `datasetId`.
    fn located(value: &serde_json::Value) -> serde_json::Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": value }] }]
        }])
    }

    /// Build the expected [`Geometry`] for a known compact-GeoJSON object by
    /// routing through the `from_geojson_value` path; the direct walker must
    /// produce an equal geometry.
    fn geometry_from_compact(compact: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(compact).expect("compact geojson decodes")
    }

    /// One `location` instance carrying `expires_at`, when given.
    fn location_instance(expires_at: Option<&str>) -> serde_json::Value {
        let mut instance = json!({ "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0]} }] });
        if let Some(raw) = expires_at {
            instance["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@value": raw}]);
        }
        instance
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
    }

    #[test]
    fn an_entity_without_a_location_has_no_location_expiry() {
        assert_eq!(extract_location_expiry(&json!([{"@id": "urn:ngsi-ld:Building:001"}])), None);
    }

    /// A `location` instance carrying `dataset_id`, with the given expiry.
    fn tagged_location_instance(dataset_id: &str, expires_at: Option<&str>) -> serde_json::Value {
        let mut instance = location_instance(expires_at);
        instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset_id}]);
        instance
    }

    #[test]
    fn a_default_location_instance_without_an_expiry_records_none() {
        let entity = json!([{"@id": "urn:x", LOCATION: [location_instance(None)]}]);
        assert_eq!(extract_location_expiry(&entity), None);
    }

    /// Clause 4.22 invalidates one instance at its own `expiresAt`, so the
    /// expiry recorded beside the hoisted default instance is that instance's
    /// own, whatever the entity's other instances carry.
    #[test]
    fn the_default_instances_own_expiry_is_recorded_whatever_the_other_instances_carry() {
        let later = json!([{"@id": "urn:x", LOCATION: [tagged_location_instance("urn:ngsi-ld:Dataset:a", Some("2031-01-01T00:00:00Z")), location_instance(Some("2030-01-01T00:00:00Z"))]}]);
        assert_eq!(extract_location_expiry(&later), Some(instant("2030-01-01T00:00:00Z")));
        let never =
            json!([{"@id": "urn:x", LOCATION: [tagged_location_instance("urn:ngsi-ld:Dataset:a", None), location_instance(Some("2030-01-01T00:00:00Z"))]}]);
        assert_eq!(extract_location_expiry(&never), Some(instant("2030-01-01T00:00:00Z")));
    }

    /// Where every instance carries a `datasetId` the first is hoisted, and its
    /// own expiry with it.
    #[test]
    fn with_every_instance_tagged_the_first_instances_expiry_is_recorded() {
        let entity = json!([{"@id": "urn:x", LOCATION: [tagged_location_instance("urn:ngsi-ld:Dataset:a", Some("2030-01-01T00:00:00Z")), tagged_location_instance("urn:ngsi-ld:Dataset:b", None)]}]);
        assert_eq!(extract_location_expiry(&entity), Some(instant("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn an_unparsable_location_expiry_counts_as_no_expiry() {
        let entity = json!([{"@id": "urn:x", LOCATION: [location_instance(Some("garbage"))]}]);
        assert_eq!(extract_location_expiry(&entity), None);
    }

    #[test]
    fn extract_geo_property_wkb_uses_direct_walker_for_expanded_form() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/location": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [
                        {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
                    ]
                }]
            }]
        }]);
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(extract_location_geometry(&entity).expect("location WKB"), geometry_from_compact(&compact));
    }

    #[test]
    fn a_geoproperty_written_as_a_literal_object_is_read_as_a_geometry() {
        // Clause 4.7 permits the GeoJSON literal to arrive as an inline object.
        let entity = located(&json!({"type": "Point", "coordinates": [13.4, 52.5]}));
        assert!(extract_location_geometry(&entity).is_some());
    }

    #[test]
    fn a_geoproperty_written_as_a_json_string_is_read_as_a_geometry() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let entity = located(&json!("{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"));
        assert!(extract_location_geometry(&entity).is_some());
    }

    #[test]
    fn an_entity_that_carries_no_such_geoproperty_yields_no_geometry() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        assert!(extract_geo_property_geometry(&entity, LOCATION).is_none());
    }

    /// Clause 4.10: a geo restriction is met when *any* instance of the target
    /// `GeoProperty` meets it, so the matching reader yields every instance —
    /// including the tagged ones the default-instance reader deliberately
    /// ignores — in payload order.
    #[test]
    fn every_instance_of_a_multi_dataset_geoproperty_is_yielded_for_matching() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:tagged"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }]
                }
            ]
        }]);
        let instances: Vec<Geometry> = geo_property_instance_geometries(&entity, LOCATION).collect();
        pretty_assertions::assert_eq!(instances.len(), 2);
        pretty_assertions::assert_eq!(instances[0], geometry_from_compact(&json!({"type": "Point", "coordinates": [0.0, 0.0]})));
        pretty_assertions::assert_eq!(instances[1], geometry_from_compact(&json!({"type": "Point", "coordinates": [13.4, 52.5]})));

        // The index-side reader still reduces the same payload to the one
        // instance clause 4.5.16.1 calls the default.
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("default instance geometry"),
            geometry_from_compact(&json!({"type": "Point", "coordinates": [13.4, 52.5]}))
        );
    }

    /// An instance whose value is not a geometry is skipped rather than
    /// stopping the walk, exactly as the SQL rendering skips an
    /// `current_attribute_values` row with a null `value_geometry`.
    #[test]
    fn an_instance_carrying_no_geometry_does_not_hide_the_instances_after_it() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                { "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": "not geojson" }] },
                { "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }] }
            ]
        }]);
        pretty_assertions::assert_eq!(geo_property_instance_geometries(&entity, LOCATION).count(), 1);
    }

    #[test]
    fn an_entity_carrying_no_such_geoproperty_yields_no_instances_for_matching() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        pretty_assertions::assert_eq!(geo_property_instance_geometries(&entity, LOCATION).count(), 0);
    }

    #[test]
    fn a_geoproperty_whose_value_is_not_geojson_yields_no_geometry() {
        let entity = located(&json!("not geojson"));
        assert!(extract_location_geometry(&entity).is_none());
    }

    #[test]
    fn the_default_instance_is_read_regardless_of_array_order() {
        // Clause 4.5.16.1: the default instance is the one without a datasetId.
        // The tagged instance is placed first to prove selection is by datasetId
        // absence, not by array position: the write/index and read/match paths
        // must pick the same instance so an index hit and a store re-check agree.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:tagged"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }]
                }
            ]
        }]);
        let compact = json!({"type": "Point", "coordinates": [13.4, 52.5]});
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("default instance geometry"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn when_every_instance_is_tagged_the_first_is_read() {
        // No datasetId-less instance exists, so no default is defined; the first
        // is taken deterministically rather than the last.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:B"}]
                }
            ]
        }]);
        let compact = json!({"type": "Point", "coordinates": [13.4, 52.5]});
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("first tagged geometry"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn the_expanded_node_form_is_read_for_an_arbitrary_geoproperty_iri() {
        // The single reader answers for any GeoProperty IRI, not only the three
        // well-known ones, and handles the fully expanded node form.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://example.com/site": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [
                        {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
                    ]
                }]
            }]
        }]);
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(
            extract_geo_property_geometry(&entity, "https://example.com/site").expect("arbitrary geoproperty"),
            geometry_from_compact(&compact)
        );
    }
}
