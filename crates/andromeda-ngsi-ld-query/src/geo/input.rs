//! Decoded NGSI-LD geo-query input before JSON-LD context expansion.

use crate::geo::reference_geometry::ReferenceGeometry;
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
use serde_json::Value;
use thiserror::Error;

/// A complete clause-4.10 geo-query whose reference geometry is read, or
/// held encoded until the query is compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQueryInput {
    /// Spatial relationship to evaluate.
    pub georel: GeoRel,
    /// The reference geometry, read or still encoded.
    pub reference: ReferenceGeometry,
    /// Compact `GeoProperty` identifier, or `location` when absent.
    pub geoproperty: Option<NgsiLdReference>,
}

/// The spec-facing members used to assemble one geo-query.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeoQueryInputWire {
    /// Spatial relationship.
    georel: GeoRel,
    /// `GeoJSON` geometry type.
    geometry: String,
    /// `GeoJSON` coordinates.
    coordinates: Value,
    /// Optional `GeoProperty` reference.
    geoproperty: Option<NgsiLdReference>,
}

impl GeoQueryInput {
    /// Construct a complete geo-query around a read reference geometry.
    #[must_use]
    pub const fn new(georel: GeoRel, geometry: Geometry, geoproperty: Option<NgsiLdReference>) -> GeoQueryInput {
        GeoQueryInput {
            georel,
            reference: ReferenceGeometry::Read(geometry),
            geoproperty,
        }
    }

    /// Decode the three wire components at request extraction.
    ///
    /// Coordinates given as JSON are read into the reference geometry now;
    /// coordinates encoded as a string (Table 5.2.13-1) are held as written,
    /// and read when the query is compiled within the broker's limits.
    ///
    /// # Errors
    ///
    /// Returns [`GeoQueryInputError::GeoRel`] when `georel` names no clause 4.10
    /// relation, [`GeoQueryInputError::GeometryType`] when `geometry_type` is
    /// not an RFC 7946 type NGSI-LD supports, and
    /// [`GeoQueryInputError::Geometry`] when JSON coordinates do not form a
    /// valid reference geometry.
    pub fn from_wire(georel: &str, geometry_type: &str, coordinates: Value, geoproperty: Option<NgsiLdReference>) -> Result<GeoQueryInput, GeoQueryInputError> {
        let georel = georel.parse().map_err(GeoQueryInputError::GeoRel)?;
        let geometry_type = geometry_type.parse::<GeometryType>().map_err(GeoQueryInputError::GeometryType)?;
        let reference = match coordinates {
            Value::String(coordinates) => ReferenceGeometry::Encoded { geometry_type, coordinates },
            read @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => {
                ReferenceGeometry::Read(Geometry::from_type_and_coordinates(geometry_type, read).map_err(GeoQueryInputError::Geometry)?)
            }
        };
        Ok(GeoQueryInput {
            georel,
            reference,
            geoproperty,
        })
    }
}

// A derive cannot combine the `geometry` and `coordinates` wire members into
// the single reference geometry held by the domain type.
impl<'de> Deserialize<'de> for GeoQueryInput {
    fn deserialize<D>(deserializer: D) -> Result<GeoQueryInput, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeoQueryInputWire::deserialize(deserializer)?;
        GeoQueryInput::from_wire(&wire.georel.to_string(), &wire.geometry, wire.coordinates, wire.geoproperty).map_err(DeserializeError::custom)
    }
}

// The domain invariant holds one reference geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl Serialize for GeoQueryInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (geometry_type, coordinates) = self.reference.wire_members().map_err(S::Error::custom)?;
        let mut state = serializer.serialize_struct("GeoQuery", if self.geoproperty.is_some() { 4 } else { 3 })?;
        state.serialize_field("georel", &self.georel)?;
        state.serialize_field("geometry", &geometry_type)?;
        state.serialize_field("coordinates", &coordinates)?;
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

#[cfg(test)]
mod tests {
    use crate::geo::{input::GeoQueryInput, reference_geometry::ReferenceGeometry};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, geometry_type::GeometryType};
    use serde_json::json;

    #[test]
    fn a_geo_query_serializes_its_geometry_as_type_and_coordinates_members() {
        let geometry: Geometry = serde_json::from_value(json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("a valid point");
        let georel: GeoRel = serde_json::from_value(json!("intersects")).expect("a valid georel");
        let serialized = serde_json::to_value(GeoQueryInput::new(georel, geometry, None)).expect("serializes");
        pretty_assertions::assert_eq!(serialized["geometry"], json!("Point"));
        pretty_assertions::assert_eq!(serialized["coordinates"], json!([1.0, 2.0]));
    }

    /// Table 5.2.13-1: coordinates encoded as a string are held as written,
    /// unread, and written back the way they arrived.
    #[test]
    fn string_coordinates_are_held_encoded_and_written_back_as_a_string() {
        let input = GeoQueryInput::from_wire("within", "Polygon", json!("[[[0,0],[1,0],[1,1],[0,0]]]"), None).expect("a decoded geo-query");
        pretty_assertions::assert_eq!(
            input.reference,
            ReferenceGeometry::Encoded {
                geometry_type: GeometryType::Polygon,
                coordinates: "[[[0,0],[1,0],[1,1],[0,0]]]".to_owned(),
            }
        );
        let serialized = serde_json::to_value(&input).expect("serializes");
        pretty_assertions::assert_eq!(serialized["coordinates"], json!("[[[0,0],[1,0],[1,1],[0,0]]]"));
    }

    /// Encoded coordinates that are no JSON text are not read at extraction;
    /// the compilation refuses them.
    #[test]
    fn string_coordinates_are_not_parsed_at_extraction() {
        assert!(GeoQueryInput::from_wire("within", "Point", json!("[1.0, "), None).is_ok());
        assert!(GeoQueryInput::from_wire("within", "Point", json!([1.0]), None).is_err());
    }
}
