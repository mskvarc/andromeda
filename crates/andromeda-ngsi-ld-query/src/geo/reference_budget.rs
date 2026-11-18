//! Bounding the reference geometry a query is resolved against.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.6: "When a query operation is so complex
//! that cannot be resolved by an NGSI-LD system, implementations shall raise
//! an error of type `TooComplexQuery`." Evaluating a geo-query or a distance
//! ordering costs work per position of its reference geometry for every
//! candidate entity, so a reference geometry holding more positions than the
//! broker resolves in one query makes the query too complex.

use andromeda_ngsi_ld_geometry::{
    geometry::Geometry,
    geometry_census::{GeometryCensusLimits, GeometryOverLimit},
};
use thiserror::Error;

/// A reference geometry of a geo-query or a distance ordering holding more
/// positions than the broker resolves in one query (clause 5.5.6,
/// `TooComplexQuery`).
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the reference geometry holds {over}; this broker resolves no query against it, send a simplified geometry")]
pub struct ReferenceBudgetError {
    /// The limit the reference geometry holds more than.
    pub over: GeometryOverLimit,
}

/// Admit a query's reference geometry holding no more positions than `limits`
/// admits.
///
/// # Errors
///
/// Returns [`ReferenceBudgetError`] for a geometry holding more positions than
/// `limits` admits.
pub fn admit_reference_geometry(geometry: &Geometry, limits: GeometryCensusLimits) -> Result<(), ReferenceBudgetError> {
    limits
        .admit_positions(geometry.form().position_count())
        .map_err(|over| ReferenceBudgetError { over })
}

#[cfg(test)]
mod tests {
    use crate::geo::reference_budget::admit_reference_geometry;
    use andromeda_ngsi_ld_geometry::{
        geometry::Geometry,
        geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use serde_json::json;
    use std::num::NonZeroUsize;

    /// A Polygon whose one ring holds `positions` positions.
    fn polygon(positions: usize) -> Geometry {
        let mut ring: Vec<serde_json::Value> = (0..positions - 1)
            .map(|index| json!([f64::from(u32::try_from(index).expect("a small fixture index")) * 0.001, 0.5]))
            .collect();
        ring.push(json!([0.0, 0.5]));
        serde_json::from_value::<Geometry>(json!({"type": "Polygon", "coordinates": [ring]})).expect("a valid polygon")
    }

    /// Limits of `positions` positions.
    fn limit(positions: usize) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive test limit")),
            values: JsonValueLimit::new(NonZeroUsize::MAX),
        }
    }

    #[test]
    fn a_reference_geometry_within_the_budget_is_admitted() {
        assert!(admit_reference_geometry(&polygon(5), limit(5)).is_ok());
    }

    #[test]
    fn a_reference_geometry_past_the_budget_is_too_complex() {
        let refused = admit_reference_geometry(&polygon(6), limit(5)).expect_err("six positions exceed five");
        assert!(matches!(refused.over, GeometryOverLimit::Positions { .. }), "{refused:?}");
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "the reference geometry holds more than the 5 positions this broker accepts in one geometry; this broker resolves no query against it, send a simplified geometry"
        );
    }
}
