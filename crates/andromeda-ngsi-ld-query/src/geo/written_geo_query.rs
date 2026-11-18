//! An NGSI-LD geo-query as the client wrote it, decoded and before JSON-LD context expansion.

use crate::geo::{geo_query_error::GeoQueryError, reference_geometry::ReferenceGeometry};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, geometry_type::GeometryType, query_coordinates::QueryCoordinates};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError, ser::SerializeStruct};
use strum::Display;

/// A complete clause-4.10 geo-query whose reference geometry is read, or
/// held encoded until the query is compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenGeoQuery {
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
struct GeoQueryMembers {
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

impl WrittenGeoQuery {
    /// Construct a complete geo-query around a read reference geometry.
    #[must_use]
    pub const fn new(georel: GeoRel, geometry: Geometry, geoproperty: Option<NgsiLdReference>) -> WrittenGeoQuery {
        WrittenGeoQuery {
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
    /// Returns [`GeoQueryError::InvalidGeorel`] when `georel` names no clause 4.10
    /// relation, [`GeoQueryError::InvalidGeometryType`] when `geometry_type` is
    /// not an RFC 7946 type NGSI-LD supports, and
    /// [`GeoQueryError::InvalidReferenceGeometry`] when JSON coordinates do not form a
    /// valid reference geometry.
    pub fn from_wire(
        georel: &str,
        geometry_type: &str,
        coordinates: QueryCoordinates,
        geoproperty: Option<NgsiLdReference>,
    ) -> Result<WrittenGeoQuery, GeoQueryError> {
        let georel = georel.parse().map_err(GeoQueryError::InvalidGeorel)?;
        let geometry_type = geometry_type.parse::<GeometryType>().map_err(GeoQueryError::InvalidGeometryType)?;
        let reference = match coordinates {
            QueryCoordinates::Text(coordinates) => ReferenceGeometry::Encoded { geometry_type, coordinates },
            QueryCoordinates::Written(written) => ReferenceGeometry::Read(written.read_as(geometry_type).map_err(GeoQueryError::InvalidReferenceGeometry)?),
        };
        Ok(WrittenGeoQuery {
            georel,
            reference,
            geoproperty,
        })
    }

    /// Read a geo-query from the text of its members as a query string carries
    /// them (Table 6.4.3.2-1), the coordinates held as the JSON text they are,
    /// which Table 5.2.13-1 lets a `GeoQuery` member carry as a string.
    ///
    /// # Errors
    ///
    /// As [`WrittenGeoQuery::from_wire`] describes.
    pub fn from_parameters(
        georel: &str,
        geometry_type: &str,
        coordinates: &str,
        geoproperty: Option<NgsiLdReference>,
    ) -> Result<WrittenGeoQuery, GeoQueryError> {
        WrittenGeoQuery::from_wire(georel, geometry_type, QueryCoordinates::Text(coordinates.to_owned()), geoproperty)
    }
}

// A derive cannot combine the `geometry` and `coordinates` wire members into
// the single reference geometry held by the domain type.
impl<'de> Deserialize<'de> for WrittenGeoQuery {
    fn deserialize<D>(deserializer: D) -> Result<WrittenGeoQuery, D::Error>
    where
        D: Deserializer<'de>,
    {
        GeoQueryMembers::deserialize(deserializer)?.into_written().map_err(DeserializeError::custom)
    }
}

impl GeoQueryMembers {
    /// The geo-query the members make, once every member Table 5.2.13-1 gives
    /// cardinality 1 is present.
    fn into_written(self) -> Result<WrittenGeoQuery, GeoQueryError> {
        let georel = self.georel.ok_or(GeoQueryError::MissingMember(RequiredMember::Georel))?;
        let geometry = self.geometry.ok_or(GeoQueryError::MissingMember(RequiredMember::Geometry))?;
        let coordinates = self.coordinates.ok_or(GeoQueryError::MissingMember(RequiredMember::Coordinates))?;
        WrittenGeoQuery::from_wire(&georel.to_string(), &geometry, coordinates, self.geoproperty)
    }
}

// The domain invariant holds one reference geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl Serialize for WrittenGeoQuery {
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

#[cfg(test)]
mod tests {
    use crate::geo::{reference_geometry::ReferenceGeometry, written_geo_query::WrittenGeoQuery};
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
        let serialized = serde_json::to_value(WrittenGeoQuery::new(georel, geometry, None)).expect("serializes");
        pretty_assertions::assert_eq!(serialized["geometry"], json!("Point"));
        pretty_assertions::assert_eq!(serialized["coordinates"], json!([1, 2]));
    }

    /// Table 5.2.13-1: coordinates encoded as a string are held as written,
    /// unread, and written back the way they arrived.
    #[test]
    fn string_coordinates_are_held_encoded_and_written_back_as_a_string() {
        let geo_query = WrittenGeoQuery::from_wire("within", "Polygon", coordinates(json!("[[[0,0],[1,0],[1,1],[0,0]]]")), None).expect("a decoded geo-query");
        pretty_assertions::assert_eq!(
            geo_query.reference,
            ReferenceGeometry::Encoded {
                geometry_type: GeometryType::Polygon,
                coordinates: "[[[0,0],[1,0],[1,1],[0,0]]]".to_owned(),
            }
        );
        let serialized = serde_json::to_value(&geo_query).expect("serializes");
        pretty_assertions::assert_eq!(serialized["coordinates"], json!("[[[0,0],[1,0],[1,1],[0,0]]]"));
    }

    /// Encoded coordinates that are no JSON text are not read at extraction;
    /// the compilation refuses them.
    #[test]
    fn string_coordinates_are_not_parsed_at_extraction() {
        assert!(WrittenGeoQuery::from_wire("within", "Point", coordinates(json!("[1, ")), None).is_ok());
        assert!(WrittenGeoQuery::from_wire("within", "Point", coordinates(json!([1])), None).is_err());
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
            let error = serde_json::from_value::<WrittenGeoQuery>(body).expect_err("an incomplete geo-query is refused");
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
        let geo_query = WrittenGeoQuery::from_wire("within", "Point", coordinates(json!([1, 2])), None).expect("a decoded geo-query");
        assert!(matches!(geo_query.reference, ReferenceGeometry::Read(_)));
    }
}
