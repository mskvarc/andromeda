//! Geometry extraction for registration creation and updates.
//!
//! The repository is forbidden from inspecting `Value`; the service layer
//! decodes inbound `GeoProperty` wrappers into a [`Geometry`] before any CSR
//! insert or update.

use crate::{csr_member::CsrMember, error::RegistrationError, registration_validation_reason::RegistrationValidationReason};
use andromeda_ngsi_ld_geometry::{
    geometry::Geometry,
    geometry_census::{GeometryCensusError, GeometryCensusLimits, compact_geometry_census, text_geometry_census},
};
use serde_json::Value;

/// The members of a registration that hold a geometry (clause 5.2.9).
const GEOMETRY_MEMBERS: [CsrMember; 3] = [CsrMember::Location, CsrMember::ObservationSpace, CsrMember::OperationSpace];

/// If `v` is a `GeoProperty` wrapper `{"type":"GeoProperty","value":{...}}`,
/// decode the inner `GeoJSON` geometry. Otherwise treat `v` itself as a raw
/// `GeoJSON` geometry.
pub fn unwrap_geo_property_to_geometry(v: &Value) -> Result<Geometry, RegistrationError> {
    let geometry = if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty")
        && let Some(inner) = v.get("value")
    {
        inner
    } else {
        v
    };
    Geometry::from_geojson_value(geometry).map_err(|source| RegistrationError::Validation(RegistrationValidationReason::GeoPropertyGeometry { source }))
}

/// Admit the geometries a registration payload writes when each stays within
/// `limits` (clause 6.3.2: a written input larger than the broker processes is
/// "Request Entity too large").
///
/// Counted on the payload as written, before anything is expanded or built
/// from it, a geometry encoded as a string from its text; a member that is no
/// geometry holds no positions and is left to the registration validation.
///
/// # Errors
///
/// Returns [`RegistrationError::GeometryTooLarge`] naming the first member
/// whose geometry holds more positions, or more other values, than `limits`
/// admits.
pub fn admit_registration_geometries(payload: &Value, limits: GeometryCensusLimits) -> Result<(), RegistrationError> {
    GEOMETRY_MEMBERS.into_iter().try_for_each(|member| {
        let Some(value) = payload.get(member.key()) else {
            return Ok(());
        };
        let geometry = match value.get("type").and_then(Value::as_str) {
            Some("GeoProperty") => value.get("value").unwrap_or(&Value::Null),
            Some(_) | None => value,
        };
        let values = limits.value_tally();
        let counted = match geometry {
            Value::String(text) => text_geometry_census(text, limits.positions, &values),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
                compact_geometry_census(geometry, limits.positions, &values)
            }
        };
        match counted {
            Ok(_) | Err(GeometryCensusError::Syntax(_)) => Ok(()),
            Err(GeometryCensusError::OverLimit(over)) => Err(RegistrationError::GeometryTooLarge { member, over }),
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        csr_member::CsrMember,
        error::RegistrationError,
        geometry::{admit_registration_geometries, unwrap_geo_property_to_geometry},
    };
    use andromeda_ngsi_ld_geometry::{
        geometry::Geometry,
        geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::json;
    use std::num::NonZeroUsize;

    fn roundtrip(geometry: &Geometry) -> serde_json::Value {
        serde_json::to_value(geometry).expect("serialize")
    }

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        let geometry = unwrap_geo_property_to_geometry(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(&geometry), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        let geometry = unwrap_geo_property_to_geometry(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(&geometry), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn a_registration_location_past_the_position_budget_is_refused() {
        let limit = GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(2).expect("a positive limit")),
            values: JsonValueLimit::new(NonZeroUsize::new(20).expect("a positive limit")),
        };
        let line = json!({"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2]]});
        let refused =
            admit_registration_geometries(&json!({"location": {"type": "GeoProperty", "value": line}}), limit).expect_err("three positions exceed two");
        assert!(
            matches!(
                refused,
                RegistrationError::GeometryTooLarge {
                    member: CsrMember::Location,
                    ..
                }
            ),
            "{refused:?}"
        );
        let refused = admit_registration_geometries(&json!({"operationSpace": line}), limit).expect_err("a bare geometry is counted too");
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
        assert!(admit_registration_geometries(&json!({"location": {"type": "Point", "coordinates": [0, 0]}}), limit).is_ok());
        assert!(admit_registration_geometries(&json!({"information": []}), limit).is_ok());
    }

    /// Clause 4.7.2: a geometry encoded as a string is counted from its text,
    /// its foreign members among the values besides its positions.
    #[test]
    fn a_string_encoded_registration_geometry_is_counted_before_it_is_parsed() {
        let limit = GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(2).expect("a positive limit")),
            values: JsonValueLimit::new(NonZeroUsize::new(20).expect("a positive limit")),
        };
        let line = json!({"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2]]}).to_string();
        let refused =
            admit_registration_geometries(&json!({"location": {"type": "GeoProperty", "value": line}}), limit).expect_err("three positions exceed two");
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
        let padded = json!({"type": "Point", "coordinates": [0, 0], "foreign": (0..40).collect::<Vec<u32>>()}).to_string();
        let refused = admit_registration_geometries(&json!({"location": padded}), limit).expect_err("forty values exceed twenty");
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
    }

    #[test]
    fn invalid_geometry_returns_error() {
        let input = json!({ "type": "Property", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        assert!(unwrap_geo_property_to_geometry(&input).is_err());
    }
}
