//! The `GeoProperty` attribute family (ETSI GS CIM 009 clause 5.2.7).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use jsonld::Expandable;
use serde::{Deserialize, Serialize, de::Error as DeserializeError};
use serde_json::{Map, Value};
use serde_with::skip_serializing_none;
use thiserror::Error;

/// NGSI-LD `GeoProperty` representation (ETSI GS CIM 009 clause 5.2.7).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:GeoProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct GeoProperty {
    /// Geometry value, held and serialised as `GeoJSON` per RFC 7946.
    #[jsonld(property = "ngsi:hasValue", coerce = "@json")]
    pub value: Geometry,
    /// Optional JSON-LD datatype for `value`.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<NgsiLdReference>,
    /// Previous geometry value emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValue", coerce = "@json")]
    pub previous_value: Option<Geometry>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

/// The member an attribute instance names its family in, the tag the derived
/// [`Attribute`](crate::attribute::instance::Attribute) decoding reads.
const TYPE_MEMBER: &str = "type";

/// The members of a `GeoProperty` instance other than its type and its
/// geometries.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeoPropertyEnvelope {
    /// Optional JSON-LD datatype for `value`.
    value_type: Option<NgsiLdReference>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    common: CommonAttributeFields,
}

impl GeoProperty {
    /// Read a `GeoProperty` instance from the members of its compact object.
    ///
    /// The geometries are read in place by the direct `GeoJSON` reader, so a
    /// geometry of millions of positions is never buffered once more per
    /// position on its way into the domain type; the remaining members are few
    /// and decode as every other family's do. The result is the one the
    /// derived decoding of the same object gives, except that a geometry
    /// carrying a JSON-LD keyword member is refused rather than read.
    ///
    /// # Errors
    ///
    /// Returns the decoding failure of a missing or malformed geometry, or of
    /// a malformed remaining member.
    pub(crate) fn from_instance(members: &Map<String, Value>) -> Result<GeoProperty, serde_json::Error> {
        let value = members
            .get(compact_properties::VALUE)
            .ok_or_else(|| serde_json::Error::missing_field(compact_properties::VALUE))
            .and_then(typed_geometry)?;
        let previous_value = match members.get(compact_properties::PREVIOUS_VALUE) {
            None | Some(Value::Null) => None,
            Some(geometry) => Some(typed_geometry(geometry)?),
        };
        // clone: the envelope's few members are decoded from a document of their own
        let envelope: Map<String, Value> = members
            .iter()
            .filter(|(member, _)| ![TYPE_MEMBER, compact_properties::VALUE, compact_properties::PREVIOUS_VALUE].contains(&member.as_str()))
            .map(|(member, value)| (member.clone(), value.clone()))
            .collect();
        let GeoPropertyEnvelope { value_type, common } = deserialize_exact(&Value::Object(envelope))?;
        Ok(GeoProperty {
            value,
            value_type,
            previous_value,
            common,
        })
    }
}

/// Why one geometry of a compact `GeoProperty` instance is not read as the
/// typed [`Geometry`].
#[derive(Debug, Error)]
enum TypedGeometryRefusal {
    /// The geometry object carries a JSON-LD keyword, which is no `GeoJSON`
    /// member.
    #[error("the geometry carries a JSON-LD keyword, which is no GeoJSON member")]
    JsonLdKeyword,
    /// The value is no geometry the typed model reads.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// Read one geometry of a `GeoProperty` instance as the typed [`Geometry`].
///
/// The typed geometry keeps a geometry's type and positions and nothing else,
/// so a member written beside them is gone once the instance is rendered
/// again. A `GeoJSON` foreign member (IETF RFC 7946 section 6.1) may go; a
/// JSON-LD keyword may not, because it is no `GeoJSON` at all but JSON-LD the
/// processor would act on -- an `@context` there is one clause 5.5.7 refuses
/// -- so a geometry carrying one is not read as typed, and the instance stays
/// as written for the screening that refuses it.
fn typed_geometry(geometry: &Value) -> Result<Geometry, serde_json::Error> {
    let read = if geometry.as_object().into_iter().flat_map(Map::keys).any(|member| member.starts_with('@')) {
        Err(TypedGeometryRefusal::JsonLdKeyword)
    } else {
        Geometry::from_geojson_value(geometry).map_err(TypedGeometryRefusal::from)
    };
    read.map_err(DeserializeError::custom)
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::{Attribute, AttributeValue};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use jsonld::Expandable;

    #[test]
    fn geo_property_round_trip_supports_previous_value() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "previousValue": { "type": "Point", "coordinates": [12.0, 51.0] }
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            panic!("expected GeoProperty");
        };
        assert!(property.previous_value.is_some());
    }

    #[test]
    fn geo_property_expanded_includes_value_type() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "valueType": "geojson:Point"
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            panic!("expected GeoProperty");
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(expanded_properties::VALUE_TYPE.as_str()));
    }

    #[test]
    fn a_geometry_carrying_a_json_ld_keyword_stays_as_written() {
        let member = serde_json::json!({
            "type": "GeoProperty",
            "value": {"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}
        });
        let decoded = AttributeValue::from_member(member.clone()); // clone: the decoded member is compared with what was written
        let AttributeValue::Raw(raw) = decoded else {
            panic!("a geometry carrying @context is not read as typed");
        };
        pretty_assertions::assert_eq!(raw, member);
    }

    #[test]
    fn a_geometry_carrying_a_geojson_foreign_member_is_read_as_typed() {
        let member = serde_json::json!({
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.5], "title": "a foreign member"}
        });
        assert!(matches!(AttributeValue::from_member(member), AttributeValue::Single(_)));
    }
}
