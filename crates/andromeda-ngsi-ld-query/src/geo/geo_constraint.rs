//! A clause 4.10 geo-query in one name stage: as the client wrote it, or after JSON-LD context expansion.

use crate::{
    geo::{geo_query_error::GeoQueryError, reference_geometry::ReferenceGeometry, required_member::RequiredMember},
    name_stage::{Expanded, NameStage, Written},
};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, geometry_type::GeometryType, query_coordinates::QueryCoordinates};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError, ser::SerializeStruct};

/// A complete clause-4.10 geo-query whose reference geometry is read, or
/// held encoded until the query is compiled.
///
/// The stage decides the spelling of the `geoproperty`: a term, a compact IRI
/// or an absolute IRI as written, an absolute IRI once expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoConstraint<S: NameStage> {
    /// Spatial relationship to evaluate.
    pub georel: GeoRel,
    /// The reference geometry, read or still encoded.
    pub reference: ReferenceGeometry,
    /// The `GeoProperty` the query reads, or `location` when absent.
    pub geoproperty: Option<S::Attribute>,
}

/// The spec-facing members used to assemble one geo-query.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeoQueryMembers<A> {
    /// Spatial relationship.
    georel: Option<GeoRel>,
    /// `GeoJSON` geometry type.
    geometry: Option<String>,
    /// `GeoJSON` coordinates.
    coordinates: Option<QueryCoordinates>,
    /// Optional `GeoProperty` reference.
    geoproperty: Option<A>,
}

impl GeoConstraint<Written> {
    /// Construct a complete geo-query around a read reference geometry.
    #[must_use]
    pub const fn new(georel: GeoRel, geometry: Geometry, geoproperty: Option<AttributeName>) -> GeoConstraint<Written> {
        GeoConstraint {
            georel,
            reference: ReferenceGeometry::Read(geometry),
            geoproperty,
        }
    }

    /// Decode the three wire components at request extraction.
    ///
    /// Coordinates given as JSON are read into the reference geometry at
    /// extraction; coordinates encoded as a string (Table 5.2.13-1) are held as written,
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
        geoproperty: Option<AttributeName>,
    ) -> Result<GeoConstraint<Written>, GeoQueryError> {
        let georel = georel.parse().map_err(GeoQueryError::InvalidGeorel)?;
        let geometry_type = geometry_type.parse::<GeometryType>().map_err(GeoQueryError::InvalidGeometryType)?;
        let reference = match coordinates {
            QueryCoordinates::Text(coordinates) => ReferenceGeometry::Encoded { geometry_type, coordinates },
            QueryCoordinates::Written(written) => ReferenceGeometry::Read(written.read_as(geometry_type).map_err(GeoQueryError::InvalidReferenceGeometry)?),
        };
        Ok(GeoConstraint {
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
    /// As [`GeoConstraint::from_wire`] describes.
    pub fn from_parameters(
        georel: &str,
        geometry_type: &str,
        coordinates: &str,
        geoproperty: Option<AttributeName>,
    ) -> Result<GeoConstraint<Written>, GeoQueryError> {
        GeoConstraint::from_wire(georel, geometry_type, QueryCoordinates::Text(coordinates.to_owned()), geoproperty)
    }
}

impl GeoConstraint<Expanded> {
    /// The same geo-query in the written stage, its `geoproperty` read as the
    /// attribute name it spells. An absolute IRI is a valid written name.
    #[must_use]
    pub fn into_written(self) -> GeoConstraint<Written> {
        GeoConstraint {
            georel: self.georel,
            reference: self.reference,
            geoproperty: self.geoproperty.map(AttributeName::from),
        }
    }
}

// A derive cannot combine the `geometry` and `coordinates` wire members into
// the single reference geometry held by the domain type.
impl<'de, S: NameStage> Deserialize<'de> for GeoConstraint<S>
where
    S::Attribute: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<GeoConstraint<S>, D::Error>
    where
        D: Deserializer<'de>,
    {
        GeoQueryMembers::<S::Attribute>::deserialize(deserializer)?
            .into_constraint()
            .map_err(DeserializeError::custom)
    }
}

impl<A> GeoQueryMembers<A> {
    /// The geo-query the members make, once every member Table 5.2.13-1 gives
    /// cardinality 1 is present.
    fn into_constraint<S: NameStage<Attribute = A>>(self) -> Result<GeoConstraint<S>, GeoQueryError> {
        let georel: GeoRel = self.georel.ok_or(GeoQueryError::MissingMember(RequiredMember::Georel))?;
        let geometry = self.geometry.ok_or(GeoQueryError::MissingMember(RequiredMember::Geometry))?;
        let coordinates = self.coordinates.ok_or(GeoQueryError::MissingMember(RequiredMember::Coordinates))?;
        let geometry_type = geometry.parse::<GeometryType>().map_err(GeoQueryError::InvalidGeometryType)?;
        let reference = match coordinates {
            QueryCoordinates::Text(coordinates) => ReferenceGeometry::Encoded { geometry_type, coordinates },
            QueryCoordinates::Written(written) => ReferenceGeometry::Read(written.read_as(geometry_type).map_err(GeoQueryError::InvalidReferenceGeometry)?),
        };
        Ok(GeoConstraint {
            georel,
            reference,
            geoproperty: self.geoproperty,
        })
    }
}

// The domain invariant holds one reference geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl<S: NameStage> Serialize for GeoConstraint<S>
where
    S::Attribute: Serialize,
{
    fn serialize<Z>(&self, serializer: Z) -> Result<Z::Ok, Z::Error>
    where
        Z: Serializer,
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
    use crate::{
        geo::{geo_constraint::GeoConstraint, reference_geometry::ReferenceGeometry},
        name_stage::{Expanded, Written},
    };
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
        let serialized = serde_json::to_value(GeoConstraint::new(georel, geometry, None)).expect("serializes");
        pretty_assertions::assert_eq!(serialized["geometry"], json!("Point"));
        pretty_assertions::assert_eq!(serialized["coordinates"], json!([1, 2]));
    }

    /// Table 5.2.13-1: coordinates encoded as a string are held as written,
    /// unread, and written back the way they arrived.
    #[test]
    fn string_coordinates_are_held_encoded_and_written_back_as_a_string() {
        let geo_query = GeoConstraint::from_wire("within", "Polygon", coordinates(json!("[[[0,0],[1,0],[1,1],[0,0]]]")), None).expect("a decoded geo-query");
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
        assert!(GeoConstraint::from_wire("within", "Point", coordinates(json!("[1, ")), None).is_ok());
        assert!(GeoConstraint::from_wire("within", "Point", coordinates(json!([1])), None).is_err());
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
            let error = serde_json::from_value::<GeoConstraint<Written>>(body).expect_err("an incomplete geo-query is refused");
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
        let geo_query = GeoConstraint::from_wire("within", "Point", coordinates(json!([1, 2])), None).expect("a decoded geo-query");
        assert!(matches!(geo_query.reference, ReferenceGeometry::Read(_)));
    }

    #[test]
    fn an_expanded_geoproperty_reads_as_an_absolute_iri_and_refuses_a_term() {
        let wire = |geoproperty: &str| json!({"georel": "within", "geometry": "Point", "coordinates": [1, 2], "geoproperty": geoproperty});
        let expanded = serde_json::from_value::<GeoConstraint<Expanded>>(wire("https://example.com/place")).expect("an absolute IRI");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&expanded).expect("serializes")["geoproperty"],
            json!("https://example.com/place")
        );
        assert!(serde_json::from_value::<GeoConstraint<Expanded>>(wire("place")).is_err());
        pretty_assertions::assert_eq!(
            serde_json::to_value(expanded.into_written()).expect("serializes")["geoproperty"],
            json!("https://example.com/place")
        );
    }
}
