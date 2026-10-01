//! Holding an entity another broker answered with to the attribute rules a
//! write to this broker is held to (ETSI GS CIM 009 v1.9.1 clause 6.3.17).
//!
//! A write never stores an attribute instance whose family forbids the
//! `unitCode` it carries (clause 4.5.2), whose `unitCode` names no UN/CEFACT
//! common code (Table 5.2.5-1), or whose `GeoProperty` value is no geometry
//! clause 4.7 admits, because admission refuses it. An entity a context source
//! answers with arrives as JSON instead, compacted, and entity decoding keeps
//! an attribute it cannot read verbatim rather than failing the entity, so
//! nothing on that path refuses such an instance. This is the reading that
//! does, over the compacted JSON itself, before anything is decoded.
//!
//! One reading serves both representations a source answers in. A current
//! entity's attribute is one instance or an array of them (clause 4.5.5); a
//! temporal entity's attribute is an array of instances (clause 4.5.7), or, in
//! the `temporalValues` form, one object or one per `datasetId` holding
//! `[value, instant]` pairs (clause 4.5.9). Every such object names its family
//! under `type` and carries its `unitCode` beside it, so each is judged the
//! same way; a `GeoProperty`'s geometry sits under `value` in the first form
//! and first in each pair of `values` in the second.
//!
//! Only top-level attributes are judged, as write admission judges them: a
//! sub-attribute is carried through as it arrived.

use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_type::GeometryType};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
};
use andromeda_unit_catalog::expanded_unit_code::{ExpandedUnitCodeError, compacted_unit_code_identity};
use serde_json::{Map, Value};
use std::{slice::from_ref, str::FromStr};
use thiserror::Error;

/// The `type` token of a `GeometryCollection`, which [`GeometryType`]
/// deliberately does not name (clause 4.7.1); a concise `GeoProperty` holding
/// one is still recognised here, so that it can be refused.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

/// Why one attribute instance of a compacted entity is refused.
#[derive(Debug, Error)]
pub enum InstanceRefusal {
    /// The instance's family defines no `unitCode` member (clauses 4.5.3,
    /// 4.5.18, 4.5.20 and 4.5.24).
    #[error("a {family} carries no 'unitCode'")]
    UnitCodeForbidden {
        /// The family the instance declares.
        family: AttributeType,
    },
    /// The instance's `unitCode` is no UN/CEFACT common code the unit catalog
    /// registers (Table 5.2.5-1).
    #[error(transparent)]
    UnitCode(#[from] ExpandedUnitCodeError),
    /// The instance is a `GeoProperty` whose geometry clause 4.7 gives the
    /// broker no reading of.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// The first attribute of a compacted entity that is refused, and why.
#[derive(Debug, Error)]
#[error("attribute '{attribute}': {source}")]
pub struct RefusedAttribute {
    /// The attribute, by the name it was answered under.
    pub attribute: AttributeName,
    /// What makes one of its instances unusable.
    #[source]
    pub source: InstanceRefusal,
}

/// The first attribute of the compacted `entity`, current or temporal, one of
/// whose instances a write to this broker would refuse.
#[must_use]
pub fn first_refused_attribute(entity: &Value) -> Option<RefusedAttribute> {
    entity.as_object()?.iter().find_map(|(member, value)| {
        let instances = match value {
            Value::Array(instances) => instances.as_slice(),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
        };
        let source = instances.iter().find_map(instance_refusal)?;
        // A member that is not an attribute name is no attribute at all, and a
        // write would have refused it before any instance of it was judged.
        let attribute = member.parse().ok()?;
        Some(RefusedAttribute { attribute, source })
    })
}

/// Why one compacted instance is refused, or `None` when it is usable or
/// declares no attribute family at all.
fn instance_refusal(instance: &Value) -> Option<InstanceRefusal> {
    let members = instance.as_object()?;
    let declared = members.get("type").and_then(Value::as_str)?;
    match AttributeType::from_str(declared) {
        Ok(family) => family_refusal(family, members),
        // Clause 4.7.3: a concise `GeoProperty` is the bare geometry.
        Err(_not_a_family) if is_geometry_token(declared) => Geometry::from_written_value(instance).err().map(InstanceRefusal::from),
        Err(_not_a_family) => None,
    }
}

/// Why an instance of `family` is refused.
fn family_refusal(family: AttributeType, instance: &Map<String, Value>) -> Option<InstanceRefusal> {
    if let Some(unit_code) = instance.get("unitCode") {
        match unit_code_admission(family) {
            UnitCodeAdmission::Forbidden => return Some(InstanceRefusal::UnitCodeForbidden { family }),
            UnitCodeAdmission::Admitted => {
                if let Err(refusal) = compacted_unit_code_identity(unit_code) {
                    return Some(refusal.into());
                }
            }
        }
    }
    match family {
        AttributeType::GeoProperty => geometries(instance)
            .find_map(|geometry| Geometry::from_written_value(geometry).err())
            .map(InstanceRefusal::from),
        AttributeType::Property
        | AttributeType::Relationship
        | AttributeType::VocabProperty
        | AttributeType::LanguageProperty
        | AttributeType::JsonProperty
        | AttributeType::ListProperty
        | AttributeType::ListRelationship => None,
    }
}

/// Every geometry a `GeoProperty` instance holds: its `value`, or the first
/// member of each `[value, instant]` pair of its `values` (clause 4.5.9).
fn geometries(instance: &Map<String, Value>) -> impl Iterator<Item = &Value> {
    let pairs = instance.get("values").and_then(Value::as_array).into_iter().flatten();
    instance
        .get("value")
        .into_iter()
        .chain(pairs.filter_map(|pair| pair.as_array().and_then(|pair| pair.first())))
}

/// Whether `declared` names a `GeoJSON` geometry, as a concise `GeoProperty`'s
/// `type` does.
fn is_geometry_token(declared: &str) -> bool {
    declared == GEOMETRY_COLLECTION_TOKEN || GeometryType::from_str(declared).is_ok()
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance_revalidation::{InstanceRefusal, first_refused_attribute};
    use andromeda_ngsi_ld_geometry::{geojson::GeometryError, linear_ring::LinearRingError, position::PositionError};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_unit_catalog::expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal};
    use serde_json::{Value, json};

    fn entity(attributes: &Value) -> Value {
        let mut document = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "scope": ["/a"]});
        if let (Some(target), Some(source)) = (document.as_object_mut(), attributes.as_object()) {
            target.extend(source.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        document
    }

    #[test]
    fn a_normalized_geo_property_past_a_pole_is_refused_with_its_position() {
        let refused = first_refused_attribute(&entity(&json!({
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 146.05]}}
        })))
        .expect("latitude 146.05 is no position");
        pretty_assertions::assert_eq!(refused.attribute.as_str(), "location");
        assert!(
            matches!(
                refused.source,
                InstanceRefusal::Geometry(GeometryError::Position { source, .. }) if source == PositionError::LatitudeOutOfRange { latitude: 146.05 }
            ),
            "got {:?}",
            refused.source
        );
        assert!(refused.to_string().contains("latitude 146.05 is outside [-90, 90]"), "{refused}");
    }

    #[test]
    fn a_concise_string_encoded_or_datasetid_geometry_past_the_axes_is_refused() {
        for attributes in [
            json!({"location": {"type": "Point", "coordinates": [181, 0]}}),
            json!({"location": {"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[181,0]}"}}),
            json!({"location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [181, 0]}, "datasetId": "urn:ngsi-ld:Dataset:a"}
            ]}),
            json!({"area": {"type": "GeometryCollection", "geometries": []}}),
        ] {
            assert!(first_refused_attribute(&entity(&attributes)).is_some(), "{attributes}");
        }
    }

    #[test]
    fn an_open_ring_is_refused_like_a_position_off_the_axes() {
        let refused = first_refused_attribute(&entity(&json!({
            "area": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}}
        })))
        .expect("the ring is open");
        assert!(
            matches!(
                refused.source,
                InstanceRefusal::Geometry(GeometryError::LinearRing {
                    source: LinearRingError::Open,
                    ..
                })
            ),
            "got {:?}",
            refused.source
        );
    }

    /// Table 5.2.5-1: the refusal a write of the same instance meets.
    #[test]
    fn a_unit_code_naming_no_common_code_is_refused() {
        let refused = first_refused_attribute(&entity(&json!({
            "speed": {"type": "Property", "value": 50, "unitCode": "km/h"}
        })))
        .expect("km/h is no common code");
        pretty_assertions::assert_eq!(refused.attribute.as_str(), "speed");
        assert!(
            matches!(
                refused.source,
                InstanceRefusal::UnitCode(ExpandedUnitCodeError::Refused(UnitCodeRefusal::NotACommonCode))
            ),
            "got {:?}",
            refused.source
        );
        pretty_assertions::assert_eq!(refused.to_string(), "attribute 'speed': 'unitCode' is not a UN/CEFACT common code");
    }

    /// Clause 4.5.3: a Relationship is unitless.
    #[test]
    fn a_unit_code_on_a_unitless_family_is_refused() {
        let refused = first_refused_attribute(&entity(&json!({
            "parkedIn": {"type": "Relationship", "object": "urn:ngsi-ld:Parking:1", "unitCode": "KGM"}
        })))
        .expect("a Relationship admits no unitCode");
        assert!(
            matches!(
                refused.source,
                InstanceRefusal::UnitCodeForbidden {
                    family: AttributeType::Relationship
                }
            ),
            "got {:?}",
            refused.source
        );
    }

    /// Clauses 4.5.7 and 4.5.9: the two temporal representations are judged
    /// like a current entity's instances.
    #[test]
    fn temporal_instances_and_temporal_values_pairs_are_judged_alike() {
        for attributes in [
            json!({"speed": [
                {"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"},
                {"type": "Property", "value": 51, "unitCode": "km/h", "observedAt": "2020-01-02T00:00:00Z"}
            ]}),
            json!({"speed": {"type": "Property", "values": [[50, "2020-01-01T00:00:00Z"]], "unitCode": "km/h"}}),
            json!({"location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 91]}, "observedAt": "2020-01-01T00:00:00Z"}
            ]}),
            json!({"location": {"type": "GeoProperty", "values": [
                [{"type": "Point", "coordinates": [0, 0]}, "2020-01-01T00:00:00Z"],
                [{"type": "Point", "coordinates": [0, 91]}, "2020-01-02T00:00:00Z"]
            ]}}),
            json!({"location": [{"type": "GeoProperty", "datasetId": "urn:ngsi-ld:Dataset:a", "values": [
                [{"type": "Point", "coordinates": [181, 0]}, "2020-01-01T00:00:00Z"]
            ]}]}),
        ] {
            assert!(first_refused_attribute(&entity(&attributes)).is_some(), "{attributes}");
        }
    }

    #[test]
    fn usable_attributes_and_other_members_are_not_refused() {
        let found = first_refused_attribute(&entity(&json!({
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [180, -90]}},
            "area": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]]}},
            "speed": {"type": "Property", "value": 10, "unitCode": "KMH"},
            "readings": {"type": "ListProperty", "valueList": [1, 2], "unitCode": "CEL"},
            "history": {"type": "Property", "values": [[1, "2020-01-01T00:00:00Z"]], "unitCode": "KMH"},
            "note": {"anything": "kept verbatim"}
        })));
        assert!(found.is_none(), "{found:?}");
    }

    /// Write admission judges top-level attributes only, and so does this.
    #[test]
    fn a_sub_attribute_is_carried_through_unjudged() {
        let found = first_refused_attribute(&entity(&json!({
            "speed": {"type": "Property", "value": 10, "accuracy": {"type": "Property", "value": 1, "unitCode": "km/h"}}
        })));
        assert!(found.is_none(), "{found:?}");
    }
}
