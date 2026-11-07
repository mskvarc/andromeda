//! Decoded NGSI-LD geo-query input before JSON-LD context expansion.

use crate::geo::reference_geometry::ReferenceGeometry;
use andromeda_ngsi_ld_geometry::{
    geo_rel::{GeoRel, GeoRelParseError},
    geojson::GeometryError,
    geometry::Geometry,
    geometry_type::GeometryType,
    query_coordinates::QueryCoordinates,
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError, ser::SerializeStruct};
use strum::Display;
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
    georel: Option<GeoRel>,
    /// `GeoJSON` geometry type.
    geometry: Option<String>,
    /// `GeoJSON` coordinates.
    coordinates: Option<QueryCoordinates>,
    /// Optional `GeoProperty` reference.
    geoproperty: Option<NgsiLdReference>,
}

/// The members Table 5.2.13-1 gives cardinality 1.
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum RequiredMember {
    /// `georel`.
    Georel,
    /// `geometry`.
    Geometry,
    /// `coordinates`.
    Coordinates,
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
    pub fn from_wire(
        georel: &str,
        geometry_type: &str,
        coordinates: QueryCoordinates,
        geoproperty: Option<NgsiLdReference>,
    ) -> Result<GeoQueryInput, GeoQueryInputError> {
        let georel = georel.parse().map_err(GeoQueryInputError::GeoRel)?;
        let geometry_type = geometry_type.parse::<GeometryType>().map_err(GeoQueryInputError::GeometryType)?;
        let reference = match coordinates {
            QueryCoordinates::Text(coordinates) => ReferenceGeometry::Encoded { geometry_type, coordinates },
            QueryCoordinates::Written(written) => ReferenceGeometry::Read(written.read_as(geometry_type).map_err(GeoQueryInputError::Geometry)?),
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
        GeoQueryInputWire::deserialize(deserializer)?.into_input().map_err(DeserializeError::custom)
    }
}

impl GeoQueryInputWire {
    /// The geo-query the members make, once every member Table 5.2.13-1 gives
    /// cardinality 1 is present.
    fn into_input(self) -> Result<GeoQueryInput, GeoQueryInputError> {
        let georel = self.georel.ok_or(GeoQueryInputError::MissingMember(RequiredMember::Georel))?;
        let geometry = self.geometry.ok_or(GeoQueryInputError::MissingMember(RequiredMember::Geometry))?;
        let coordinates = self.coordinates.ok_or(GeoQueryInputError::MissingMember(RequiredMember::Coordinates))?;
        GeoQueryInput::from_wire(&georel.to_string(), &geometry, coordinates, self.geoproperty)
    }
}

// The domain invariant holds one reference geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl Serialize for GeoQueryInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("GeoQuery", if self.geoproperty.is_some() { 4 } else { 3 })?;
        state.serialize_field("georel", &self.georel)?;
        state.serialize_field("geometry", &self.reference.geometry_type())?;
        state.serialize_field("coordinates", &self.reference.coordinates_member())?;
        if let Some(geoproperty) = &self.geoproperty {
            state.serialize_field("geoproperty", geoproperty)?;
        }
        state.end()
    }
}

/// Geo-query wire decoding failure.
#[derive(Debug, Error)]
pub enum GeoQueryInputError {
    /// A member Table 5.2.13-1 gives cardinality 1 is absent or `null`.
    #[error("a geo-query requires its '{0}' member (Table 5.2.13-1)")]
    MissingMember(RequiredMember),
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
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, geometry_type::GeometryType, query_coordinates::QueryCoordinates};
    use serde_json::{Value, json};

    /// The coordinates a request spells as `value`.
    fn coordinates(value: Value) -> QueryCoordinates {
        serde_json::from_value(value).expect("coordinates read from JSON")
    }

    #[test]
    fn a_geo_query_serializes_its_geometry_as_type_and_coordinates_members() {
        let geometry: Geometry = serde_json::from_value(json!({"type": "Point", "coordinates": [1, 2]})).expect("a valid point");
        let georel: GeoRel = serde_json::from_value(json!("intersects")).expect("a valid georel");
        let serialized = serde_json::to_value(GeoQueryInput::new(georel, geometry, None)).expect("serializes");
        pretty_assertions::assert_eq!(serialized["geometry"], json!("Point"));
        pretty_assertions::assert_eq!(serialized["coordinates"], json!([1, 2]));
    }

    /// Table 5.2.13-1: coordinates encoded as a string are held as written,
    /// unread, and written back the way they arrived.
    #[test]
    fn string_coordinates_are_held_encoded_and_written_back_as_a_string() {
        let input = GeoQueryInput::from_wire("within", "Polygon", coordinates(json!("[[[0,0],[1,0],[1,1],[0,0]]]")), None).expect("a decoded geo-query");
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
        assert!(GeoQueryInput::from_wire("within", "Point", coordinates(json!("[1, ")), None).is_ok());
        assert!(GeoQueryInput::from_wire("within", "Point", coordinates(json!([1])), None).is_err());
    }

    /// Table 5.2.13-1 gives `georel`, `geometry` and `coordinates` cardinality
    /// 1: a geo-query missing one is refused, and the refusal names the member
    /// in single quotes and the table.
    #[test]
    fn a_geo_query_missing_a_required_member_is_refused_by_name() {
        for (body, member) in [
            (json!({"georel": "near;maxDistance==2000"}), "geometry"),
            (json!({"georel": "within", "geometry": "Point"}), "coordinates"),
            (json!({"geometry": "Point", "coordinates": [1, 2]}), "georel"),
        ] {
            let error = serde_json::from_value::<GeoQueryInput>(body).expect_err("an incomplete geo-query is refused");
            let message = error.to_string();
            assert!(
                message.contains(&format!("'{member}'")) && message.contains("Table 5.2.13-1"),
                "{member}: {message}"
            );
        }
    }

    /// Coordinates given as a JSON array are read into the reference geometry
    /// at extraction, never held as text.
    #[test]
    fn array_coordinates_are_read_into_the_reference_geometry() {
        let input = GeoQueryInput::from_wire("within", "Point", coordinates(json!([1, 2])), None).expect("a decoded geo-query");
        assert!(matches!(input.reference, ReferenceGeometry::Read(_)));
    }
}
