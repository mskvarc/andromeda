//! The geometry members of a registration body.
//!
//! A registration writes `location`, `observationSpace` and `operationSpace` as
//! a `GeoJSON` geometry or as the `GeoProperty` carrying one (clause 5.2.9). The
//! body reads either into a [`Geometry`], and the budget a geometry is admitted
//! under (clause 6.3.2) is checked on the geometry the body holds.

use crate::{csr_member::CsrMember, error::RegistrationError};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_geometry::{
    geometry::Geometry,
    geometry_census::{GeometryCensusLimits, GeometryOverLimit},
};
use andromeda_ngsi_ld_number::json_value::value_census::{ValueCensusError, count_values_within};
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};

/// The `type` a `GeoProperty` carrying a geometry is written with.
const GEO_PROPERTY_TYPE: &str = "GeoProperty";

/// A geometry member as a registration body writes it (clause 5.2.9): the
/// `GeoJSON` geometry itself, or the `GeoProperty` whose `value` is one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RegistrationGeometry(Geometry);

impl RegistrationGeometry {
    /// The geometry the member spells.
    #[must_use]
    pub(crate) fn into_geometry(self) -> Geometry {
        self.0
    }

    /// The geometry the member spells, borrowed.
    #[must_use]
    pub(crate) const fn geometry(&self) -> &Geometry {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RegistrationGeometry {
    /// Read the member as the `GeoProperty` it is when it names that type and
    /// carries a `value`, and as a bare geometry otherwise.
    ///
    /// The member is held as admitted text while its form is decided, because a
    /// geometry object does not say which form it has until its `type` is
    /// read, whichever order its members were written in.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RegistrationGeometry, D::Error> {
        let written = JsonLiteral::deserialize(deserializer)?;
        let geometry = geo_property_value(written.piece()).unwrap_or_else(|| written.piece());
        serde_json::from_str::<Geometry>(geometry.text())
            .map(RegistrationGeometry)
            .map_err(DeserializeError::custom)
    }
}

/// The `value` of `member` when it is a `GeoProperty`.
fn geo_property_value(member: JsonPiece<'_>) -> Option<JsonPiece<'_>> {
    let named = member.member("type")?.as_str()?;
    (named == GEO_PROPERTY_TYPE).then(|| member.member("value")).flatten()
}

/// Admit the geometries a registration body writes when each stays within
/// `limits` (clause 6.3.2: a written input larger than the broker processes is
/// "Request Entity too large").
///
/// A geometry is counted for the positions it holds and for the JSON values it
/// holds besides them: its `bbox` and the foreign members IETF RFC 7946 section
/// 6.1 allows.
///
/// # Errors
///
/// Returns [`RegistrationError::GeometryTooLarge`] naming the first member
/// whose geometry holds more positions, or more other values, than `limits`
/// admits.
pub(crate) fn admit_registration_geometries<'a>(
    members: impl IntoIterator<Item = (CsrMember, &'a Geometry)>,
    limits: GeometryCensusLimits,
) -> Result<(), RegistrationError> {
    members
        .into_iter()
        .try_for_each(|(member, geometry)| admit_geometry(geometry, limits).map_err(|over| RegistrationError::GeometryTooLarge { member, over }))
}

/// Admit one geometry under `limits`.
fn admit_geometry(geometry: &Geometry, limits: GeometryCensusLimits) -> Result<(), GeometryOverLimit> {
    limits.admit_positions(geometry.form().position_count())?;
    let Some(members) = geometry.members() else {
        return Ok(());
    };
    let bbox = members.bbox().map_or(0, |bbox| 1 + bbox.ordinates().count());
    let mut values = bbox;
    for (_, foreign) in members.foreign().iter() {
        values = values.saturating_add(1).saturating_add(foreign_values(foreign, limits)?);
        if values > limits.values.get() {
            return Err(GeometryOverLimit::Values { limit: limits.values });
        }
    }
    Ok(())
}

/// The JSON values one foreign member holds, refused once it holds more than
/// the limit alone.
fn foreign_values(foreign: &JsonLiteral, limits: GeometryCensusLimits) -> Result<usize, GeometryOverLimit> {
    match count_values_within(foreign.text().as_bytes(), limits.values) {
        Ok(counted) => Ok(counted.get()),
        Err(ValueCensusError::OverLimit { limit }) => Err(GeometryOverLimit::Values { limit }),
        // A literal is admitted text, so the reader never refuses its syntax.
        Err(ValueCensusError::Syntax(_)) => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        csr_member::CsrMember,
        error::RegistrationError,
        geometry::{RegistrationGeometry, admit_registration_geometries},
    };
    use andromeda_ngsi_ld_geometry::{
        geometry::Geometry,
        geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::json;
    use std::num::NonZeroUsize;

    fn read(member: &serde_json::Value) -> Result<Geometry, serde_json::Error> {
        serde_json::from_str::<RegistrationGeometry>(&member.to_string()).map(RegistrationGeometry::into_geometry)
    }

    fn limits(positions: usize, values: usize) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit")),
            values: JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive limit")),
        }
    }

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]} });
        let geometry = read(&input).expect("decode");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&geometry).expect("serialize"),
            json!({"type": "Point", "coordinates": [1, 2]})
        );
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1, 2]});
        let geometry = read(&input).expect("decode");
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialize"), input);
    }

    #[test]
    fn invalid_geometry_returns_error() {
        let input = json!({ "type": "Property", "value": {"type": "Point", "coordinates": [1, 2]} });
        assert!(read(&input).is_err());
    }

    /// A refused geometry names its own refusal, such as the position that lies
    /// off the CRS84 axes, as it does anywhere a geometry is read.
    #[test]
    fn a_refused_geometry_names_why() {
        let error = read(&json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 91]}})).expect_err("latitude 91");
        assert!(error.to_string().contains("latitude 91 is outside [-90, 90]"), "{error}");
    }

    /// Clause 4.7.2 admits a geometry as a string only for a `GeoProperty` the
    /// entity model reads; a registration holds the geometry object itself.
    #[test]
    fn a_geometry_written_as_a_string_is_no_registration_geometry() {
        let encoded = serde_json::Value::String(json!({"type": "Point", "coordinates": [1, 2]}).to_string());
        assert!(read(&encoded).is_err());
    }

    #[test]
    fn a_registration_location_past_the_position_budget_is_refused() {
        let line = read(&json!({"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2]]})).expect("a line");
        let refused = admit_registration_geometries([(CsrMember::Location, &line)], limits(2, 20)).expect_err("three positions exceed two");
        assert!(
            matches!(
                refused,
                RegistrationError::GeometryTooLarge {
                    member: CsrMember::Location,
                    over: GeometryOverLimit::Positions { .. }
                }
            ),
            "{refused:?}"
        );
        let refused = admit_registration_geometries([(CsrMember::OperationSpace, &line)], limits(2, 20)).expect_err("a bare geometry is counted too");
        assert!(
            matches!(
                refused,
                RegistrationError::GeometryTooLarge {
                    member: CsrMember::OperationSpace,
                    ..
                }
            ),
            "{refused:?}"
        );
        let point = read(&json!({"type": "Point", "coordinates": [0, 0]})).expect("a point");
        assert!(admit_registration_geometries([(CsrMember::Location, &point)], limits(2, 20)).is_ok());
        assert!(admit_registration_geometries([], limits(2, 20)).is_ok());
    }

    /// IETF RFC 7946 section 6.1: the foreign members of a geometry cost the
    /// values they hold besides its positions.
    #[test]
    fn a_registration_geometry_past_the_value_budget_is_refused() {
        let padded = read(&json!({"type": "Point", "coordinates": [0, 0], "foreign": (0..40).collect::<Vec<u32>>()})).expect("a padded point");
        let refused = admit_registration_geometries([(CsrMember::Location, &padded)], limits(2, 20)).expect_err("forty values exceed twenty");
        assert!(
            matches!(
                refused,
                RegistrationError::GeometryTooLarge {
                    over: GeometryOverLimit::Values { .. },
                    ..
                }
            ),
            "{refused:?}"
        );
        assert!(admit_registration_geometries([(CsrMember::Location, &padded)], limits(2, 100)).is_ok());
    }
}
