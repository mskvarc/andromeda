//! Decoded NGSI-LD geo-query input before JSON-LD context expansion.

use andromeda_ngsi_ld_geometry::{
    geo_rel::{GeoRel, GeoRelParseError},
    geojson::GeometryError,
    geometry::Geometry,
    geometry_type::GeometryType,
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::Error as DeserializeError,
    ser::{Error as SerializeError, SerializeStruct},
};
use thiserror::Error;

/// A complete clause-4.10 geo-query with a validated reference geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQueryInput {
    /// Spatial relationship to evaluate.
    pub georel: GeoRel,
    /// Validated `GeoJSON` reference geometry.
    pub geometry: Geometry,
    /// Compact `GeoProperty` identifier, or `location` when absent.
    pub geoproperty: Option<NgsiLdReference>,
}

/// The spec-facing members used to assemble one validated geo-query.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeoQueryInputWire {
    /// Spatial relationship.
    georel: GeoRel,
    /// `GeoJSON` geometry type.
    geometry: String,
    /// `GeoJSON` coordinates.
    coordinates: serde_json::Value,
    /// Optional `GeoProperty` reference.
    geoproperty: Option<NgsiLdReference>,
}

impl GeoQueryInput {
    /// Construct a complete decoded geo-query.
    #[must_use]
    pub const fn new(georel: GeoRel, geometry: Geometry, geoproperty: Option<NgsiLdReference>) -> GeoQueryInput {
        GeoQueryInput { georel, geometry, geoproperty }
    }

    /// Decode the three wire components exactly once at request extraction.
    ///
    /// # Errors
    ///
    /// Returns [`GeoQueryInputError::GeoRel`] when `georel` names no clause 4.10
    /// relation, [`GeoQueryInputError::Coordinates`] when a string-encoded
    /// `coordinates` member is not JSON, [`GeoQueryInputError::GeometryType`]
    /// when `geometry_type` is not an RFC 7946 type NGSI-LD supports, and
    /// [`GeoQueryInputError::Geometry`] when the two do not form a valid
    /// reference geometry.
    pub fn from_wire(
        georel: &str,
        geometry_type: &str,
        coordinates: serde_json::Value,
        geoproperty: Option<NgsiLdReference>,
    ) -> Result<GeoQueryInput, GeoQueryInputError> {
        let georel = georel.parse().map_err(GeoQueryInputError::GeoRel)?;
        let coordinates = match coordinates {
            serde_json::Value::String(value) => serde_json::from_str(&value).map_err(GeoQueryInputError::Coordinates)?,
            value @ (serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Object(_)) => value,
        };
        let geometry_type = geometry_type.parse::<GeometryType>().map_err(GeoQueryInputError::GeometryType)?;
        let geometry = Geometry::from_type_and_coordinates(geometry_type, coordinates).map_err(GeoQueryInputError::Geometry)?;
        Ok(GeoQueryInput::new(georel, geometry, geoproperty))
    }
}

// A derive cannot combine the `geometry` and `coordinates` wire members into
// the single validated `Geometry` held by the domain type.
impl<'de> Deserialize<'de> for GeoQueryInput {
    fn deserialize<D>(deserializer: D) -> Result<GeoQueryInput, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeoQueryInputWire::deserialize(deserializer)?;
        GeoQueryInput::from_wire(&wire.georel.to_string(), &wire.geometry, wire.coordinates, wire.geoproperty).map_err(DeserializeError::custom)
    }
}

// The domain invariant stores one validated geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl Serialize for GeoQueryInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let geometry = serde_json::to_value(&self.geometry).map_err(S::Error::custom)?;
        let object = geometry.as_object().ok_or_else(|| S::Error::custom(GeometrySerializationError::NotAnObject))?;
        let geometry_type = object.get("type").ok_or_else(|| S::Error::custom(GeometrySerializationError::NoType))?;
        let coordinates = object
            .get("coordinates")
            .ok_or_else(|| S::Error::custom(GeometrySerializationError::NoCoordinates))?;
        let mut state = serializer.serialize_struct("GeoQuery", if self.geoproperty.is_some() { 4 } else { 3 })?;
        state.serialize_field("georel", &self.georel)?;
        state.serialize_field("geometry", geometry_type)?;
        state.serialize_field("coordinates", coordinates)?;
        if let Some(geoproperty) = &self.geoproperty {
            state.serialize_field("geoproperty", geoproperty)?;
        }
        state.end()
    }
}

/// Geo-query wire decoding failure.
#[derive(Debug, Error)]
pub enum GeoQueryInputError {
    /// The relation is not defined by clause 4.10.
    #[error("invalid georel: {0}")]
    GeoRel(GeoRelParseError),
    /// The URL coordinate member was not JSON.
    #[error("invalid geo-query coordinates: {0}")]
    Coordinates(serde_json::Error),
    /// The geometry name is not one of the `GeoJSON` types supported by NGSI-LD.
    #[error("invalid geo-query geometry type: {0}")]
    GeometryType(strum::ParseError),
    /// The reference geometry is invalid.
    ///
    /// Carries the geometry diagnostic verbatim so that a position with an
    /// ordinate count RFC 7946 section 3.1.1 gives no meaning to reaches the
    /// client as a `BadRequestData` detail naming that count, rather than as a
    /// bare "invalid geometry".
    #[error("geo-query reference geometry: {0}")]
    Geometry(GeometryError),
}

/// Why a validated reference geometry could not be split into the two members
/// Table 5.2.13-1 serialises it as.
///
/// Each is a broken invariant of the geometry type's own serialisation rather
/// than anything a client sent.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
enum GeometrySerializationError {
    /// The geometry did not serialise as a JSON object.
    #[error("validated geometry did not serialize as an object")]
    NotAnObject,
    /// The geometry's object carried no `type` member.
    #[error("validated geometry serialized without a type")]
    NoType,
    /// The geometry's object carried no `coordinates` member.
    #[error("validated geometry serialized without coordinates")]
    NoCoordinates,
}

#[cfg(test)]
mod tests {
    use crate::geo::input::GeoQueryInput;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use serde_json::json;

    #[test]
    fn a_geo_query_serializes_its_geometry_as_type_and_coordinates_members() {
        let geometry: Geometry = serde_json::from_value(json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("a valid point");
        let georel: GeoRel = serde_json::from_value(json!("intersects")).expect("a valid georel");
        let serialized = serde_json::to_value(GeoQueryInput::new(georel, geometry, None)).expect("serializes");
        pretty_assertions::assert_eq!(serialized["geometry"], json!("Point"));
        pretty_assertions::assert_eq!(serialized["coordinates"], json!([1.0, 2.0]));
    }
}
