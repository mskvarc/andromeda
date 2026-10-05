//! The reference geometry of a geo-query as the request gave it.
//!
//! Table 5.2.13-1 lets a geo-query's `coordinates` be "encoded as a string"
//! for the sake of JSON-LD compatibility, and a query's URL parameters always
//! carry them as one. Parsing that text builds a tree whose cost follows the
//! values it encodes, not the request's own value count, so the text is kept
//! as written until the query is compiled: compilation knows how large a
//! reference geometry the broker resolves (clause 5.5.6), counts the text
//! against that before parsing it, and refuses one too large without having
//! built it. Coordinates given as JSON were counted with the request, and are
//! read into a geometry when the request is decoded.

use crate::{error::QueryError, geo::reference_budget::admit_reference_geometry};
use andromeda_ngsi_ld_geometry::{
    geometry::Geometry,
    geometry_census::{GeometryCensusError, GeometryCensusLimits, text_coordinates_census},
    geometry_type::GeometryType,
};
use serde_json::Value;

/// A geo-query's reference geometry, read or still encoded.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceGeometry {
    /// Coordinates given as JSON, read into a geometry when the request was
    /// decoded.
    Read(Geometry),
    /// Coordinates encoded as a JSON string, held as written until the query
    /// is compiled within the broker's limits.
    Encoded {
        /// The `GeoJSON` geometry type the coordinates are read as.
        geometry_type: GeometryType,
        /// The JSON text of the coordinates.
        coordinates: String,
    },
}

impl ReferenceGeometry {
    /// The geometry, admitted within `limits`: a read one by its positions,
    /// an encoded one counted from its text before it is parsed.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ReferenceGeometryTooComplex`] for a geometry past
    /// `limits` (clause 5.5.6), [`QueryError::GeoCoordinatesInvalid`] for
    /// encoded coordinates that are no JSON text, and
    /// [`QueryError::ReferenceGeometry`] for coordinates that do not form the
    /// named geometry.
    pub fn admit(&self, limits: GeometryCensusLimits) -> Result<Geometry, QueryError> {
        match self {
            ReferenceGeometry::Read(geometry) => {
                admit_reference_geometry(geometry, limits)?;
                // The compiled query owns its geometry apart from the reusable request.
                Ok(geometry.clone())
            }
            ReferenceGeometry::Encoded { geometry_type, coordinates } => {
                text_coordinates_census(coordinates, limits).map_err(|refusal| match refusal {
                    GeometryCensusError::OverLimit(over) => QueryError::ReferenceGeometryTooComplex { over },
                    GeometryCensusError::Syntax(source) => QueryError::GeoCoordinatesInvalid(source),
                })?;
                let parsed = serde_json::from_str::<Value>(coordinates).map_err(QueryError::GeoCoordinatesInvalid)?;
                Ok(Geometry::from_type_and_coordinates(*geometry_type, parsed)?)
            }
        }
    }

    /// The geometry type and the coordinates as Table 5.2.13-1 writes them:
    /// a read geometry's coordinates as JSON, an encoded one's as the string
    /// it arrived as.
    ///
    /// # Errors
    ///
    /// Returns the `serde_json` failure of rendering a read geometry, which
    /// is a broken invariant of its own serialisation.
    pub fn wire_members(&self) -> Result<(GeometryType, Value), serde_json::Error> {
        match self {
            ReferenceGeometry::Read(geometry) => {
                let geometry_type = geometry.form().geometry_type();
                let Value::Object(mut members) = geometry.to_geojson_value()? else {
                    return Ok((geometry_type, Value::Null));
                };
                Ok((geometry_type, members.remove("coordinates").unwrap_or(Value::Null)))
            }
            // clone: the wire member owns its text apart from the held request
            ReferenceGeometry::Encoded { geometry_type, coordinates } => Ok((*geometry_type, Value::String(coordinates.clone()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::QueryError, geo::reference_geometry::ReferenceGeometry};
    use andromeda_ngsi_ld_geometry::{
        geometry::Geometry,
        geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit},
        geometry_type::GeometryType,
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// Limits of `positions` positions and a hundred other values.
    fn limits(positions: usize) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive test limit")),
            values: JsonValueLimit::new(NonZeroUsize::new(100).expect("a positive test limit")),
        }
    }

    /// The coordinates of a `LineString` of `positions` positions.
    fn line(positions: usize) -> Value {
        (0..positions)
            .map(|index| json!([f64::from(u32::try_from(index).expect("a small fixture index")) * 0.01, 1.0]))
            .collect()
    }

    /// `coordinates` encoded as the `LineString` reference of a query.
    fn encoded(coordinates: &str) -> ReferenceGeometry {
        ReferenceGeometry::Encoded {
            geometry_type: GeometryType::LineString,
            coordinates: coordinates.to_owned(),
        }
    }

    #[test]
    fn encoded_coordinates_within_the_limits_read_as_the_geometry_they_encode() {
        let admitted = encoded(&line(4).to_string()).admit(limits(4)).expect("four positions are within four");
        let read = Geometry::from_type_and_coordinates(GeometryType::LineString, line(4)).expect("a line");
        pretty_assertions::assert_eq!(admitted, read);
    }

    /// Clause 5.5.6: a reference geometry past the limits is `TooComplexQuery`,
    /// refused before its text is parsed.
    #[test]
    fn encoded_coordinates_past_the_limits_are_too_complex() {
        let refused = encoded(&line(5).to_string()).admit(limits(4)).expect_err("five positions exceed four");
        assert!(
            matches!(
                refused,
                QueryError::ReferenceGeometryTooComplex {
                    over: GeometryOverLimit::Positions { .. }
                }
            ),
            "{refused:?}"
        );
        let empties = Value::Array(vec![json!([]); 200]).to_string();
        let refused = encoded(&empties).admit(limits(4)).expect_err("two hundred lists exceed a hundred values");
        assert!(
            matches!(
                refused,
                QueryError::ReferenceGeometryTooComplex {
                    over: GeometryOverLimit::Values { .. }
                }
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn encoded_coordinates_that_are_no_json_or_no_geometry_are_refused() {
        let refused = encoded("[[0, 0], ").admit(limits(4)).expect_err("no JSON text");
        assert!(matches!(refused, QueryError::GeoCoordinatesInvalid(_)), "{refused:?}");
        let refused = encoded("[[0, 0]]").admit(limits(4)).expect_err("one position traces no line");
        assert!(matches!(refused, QueryError::ReferenceGeometry(_)), "{refused:?}");
    }

    #[test]
    fn a_read_geometry_is_admitted_by_its_positions() {
        let read = ReferenceGeometry::Read(Geometry::from_type_and_coordinates(GeometryType::LineString, line(5)).expect("a line"));
        assert!(read.admit(limits(5)).is_ok());
        assert!(matches!(read.admit(limits(4)), Err(QueryError::ReferenceGeometryTooComplex { .. })));
    }

    /// Table 5.2.13-1: the members are written back as they arrived.
    #[test]
    fn the_wire_members_keep_the_form_the_coordinates_arrived_in() {
        let text = line(2).to_string();
        pretty_assertions::assert_eq!(encoded(&text).wire_members().expect("renders"), (GeometryType::LineString, Value::String(text)));
        let read = ReferenceGeometry::Read(Geometry::from_type_and_coordinates(GeometryType::Point, json!([1.0, 2.0])).expect("a point"));
        pretty_assertions::assert_eq!(read.wire_members().expect("renders"), (GeometryType::Point, json!([1.0, 2.0])));
    }
}
