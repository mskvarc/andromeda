//! The `GeoProperty` attribute family (ETSI GS CIM 009 clause 5.2.7).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_members::JsonLdKeywords, written_geometry::WrittenGeometry};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::{
    Deserialize,
    Serialize,
    de::{Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, Visitor},
};
use serde_json::{Map, Value};
use serde_with::skip_serializing_none;
use std::fmt;
use thiserror::Error;

/// NGSI-LD `GeoProperty` representation (ETSI GS CIM 009 clause 5.2.7).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoProperty {
    /// Geometry value, held and serialised as `GeoJSON` per RFC 7946.
    pub value: Geometry,
    /// Optional JSON-LD datatype for `value`.
    pub value_type: Option<NgsiLdReference>,
    /// Previous geometry value emitted in notifications when `showChanges` is active.
    pub previous_value: Option<Geometry>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

/// The member an attribute instance names its family in, the tag the derived
/// [`Attribute`](crate::attribute::instance::Attribute) decoding reads.
pub(crate) const TYPE_MEMBER: &str = "type";

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
    /// Read a `GeoProperty` instance from its compact object, its own `type`
    /// member skipped.
    ///
    /// The geometries are read straight from the deserializer, so a geometry
    /// of millions of positions is never held as a JSON tree on its way into
    /// the domain type; the remaining members are few and decode as every
    /// other family's do. The result is the one the derived decoding of the
    /// same object gives, except that a geometry carrying a JSON-LD keyword
    /// member is refused rather than read. A member written twice counts as
    /// written last, as a parsed tree keeps it.
    ///
    /// # Errors
    ///
    /// Returns the decoding failure of a missing or malformed geometry, or of
    /// a malformed remaining member.
    pub(crate) fn from_instance<'de, D: Deserializer<'de>>(instance: D) -> Result<GeoProperty, D::Error> {
        instance.deserialize_map(GeoPropertyVisitor)
    }
}

/// Reads the members of one `GeoProperty` instance.
#[derive(Clone, Copy, Debug)]
struct GeoPropertyVisitor;

impl<'de> Visitor<'de> for GeoPropertyVisitor {
    type Value = GeoProperty;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GeoProperty instance")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<GeoProperty, A::Error> {
        let mut value = None;
        let mut previous_value = None;
        let mut envelope = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            match name.as_str() {
                TYPE_MEMBER => {
                    members.next_value::<IgnoredAny>()?;
                }
                compact_properties::VALUE => value = Some(typed_geometry(members.next_value()?)),
                compact_properties::PREVIOUS_VALUE => previous_value = Some(members.next_value::<Option<WrittenGeometry>>()?.map(typed_geometry)),
                _ => {
                    envelope.insert(name, members.next_value::<Value>()?);
                }
            }
        }
        let value = value
            .ok_or_else(|| A::Error::missing_field(compact_properties::VALUE))?
            .map_err(A::Error::custom)?;
        let previous_value = previous_value.flatten().transpose().map_err(A::Error::custom)?;
        let GeoPropertyEnvelope { value_type, common } = deserialize_exact(&Value::Object(envelope)).map_err(A::Error::custom)?;
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
pub enum TypedGeometryRefusal {
    /// The geometry object carries a JSON-LD keyword, which is no `GeoJSON`
    /// member.
    #[error("the geometry carries a JSON-LD keyword, which is no GeoJSON member")]
    JsonLdKeyword,
    /// The value is no geometry the typed model reads.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// Read one written geometry of a `GeoProperty` instance as the typed
/// [`Geometry`].
///
/// The typed geometry keeps a geometry's type and positions and nothing else,
/// so a member written beside them is gone once the instance is rendered
/// again. A `GeoJSON` foreign member (IETF RFC 7946 section 6.1) may go; a
/// JSON-LD keyword may not, because it is no `GeoJSON` at all but JSON-LD the
/// processor would act on -- an `@context` there is one clause 5.5.7 refuses
/// -- so a geometry carrying one is not read as typed, and the instance stays
/// as written for the screening that refuses it.
pub(crate) fn typed_geometry(written: WrittenGeometry) -> Result<Geometry, TypedGeometryRefusal> {
    match written.json_ld_keywords() {
        JsonLdKeywords::Present => Err(TypedGeometryRefusal::JsonLdKeyword),
        JsonLdKeywords::Absent => Ok(Geometry::new(written.into_form()?)),
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::{Attribute, AttributeValue};

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
