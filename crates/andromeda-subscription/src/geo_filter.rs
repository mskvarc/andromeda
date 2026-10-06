//! A subscription's `geoQ` (ETSI GS CIM 009 v1.9.1 clause 5.2.13), admitted
//! once on its way in and compiled once on its way to matching.
//!
//! Table 5.2.13-1 gives `georel`, `geometry` and `coordinates` cardinality 1
//! and `geoproperty` 0..1, defaulting to `location`; clause 4.10 needs all
//! three "to fully specify an NGSI-LD Geoquery". The request model reads the
//! `geoQ` as a [`GeoQueryInput`], which refuses one that names fewer than the
//! three or whose members are no relation, geometry type or geometry, so what
//! this module judges is what that reading cannot: whether the reference
//! geometry is within the limits the broker processes (clause 6.3.2: a written
//! input larger than the broker processes is "Request Entity too large"), and
//! whether the coordinates a string encodes (Table 5.2.13-1: "For the sake of
//! JSON-LD compatibility It can be encoded as a string") are a geometry.
//! [`admit_geo_q`] counts string-encoded coordinates before they are parsed.
//! [`compile_geo_q`] reads the same `geoQ` for the matcher.
//!
//! A stored `geoQ` that no longer compiles, one written before a rule it
//! breaks existed, restricts matching to nothing. Clause 5.8.6 includes the
//! entities that match the geoquery, and all of them only "if query, geoquery
//! and scopeQuery are all not defined"; a defined geoquery the broker cannot
//! evaluate admits no entity, and widening it into none would notify a
//! subscriber of everything.

use crate::{error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry_census::GeometryCensusLimits, query_coordinates::WrittenCoordinates};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    geo::{input::GeoQueryInput, query::GeoQuery, reference_geometry::ReferenceGeometry},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use thiserror::Error;
use tracing::warn;

/// Why a `geoQ` is not a `GeoQuery` the broker can evaluate.
#[derive(Debug, Error)]
pub enum GeoQError {
    /// The reference geometry was refused when it was admitted.
    #[error("geoQ reference geometry: {source}")]
    Reference {
        /// What makes the reference geometry unusable.
        #[source]
        source: Box<QueryError>,
    },
    /// The `coordinates` member was a string that was not JSON.
    #[error("geoQ coordinates: {source}")]
    Coordinates {
        /// The JSON parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// The `geometry` and `coordinates` did not form a usable RFC 7946
    /// geometry.
    #[error("geoQ reference geometry: {source}")]
    ReferenceGeometry {
        /// What makes the geometry unusable, down to the refused position.
        #[source]
        source: GeometryError,
    },
    /// The `geoproperty` was not an attribute name.
    #[error("geoQ geoproperty: {source}")]
    Geoproperty {
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
}

/// Read a `geoQ` into the request-side [`GeoQuery`] clause 4.10 evaluates.
///
/// # Errors
///
/// Returns the first [`GeoQError`] the `geoQ` meets: coordinates a string
/// encodes that are no JSON or no geometry, or a `geoproperty` that is no
/// attribute name.
pub fn compile_geo_q(geo_q: &GeoQueryInput) -> Result<GeoQuery, GeoQError> {
    let geometry = match &geo_q.reference {
        // clone: the compiled query owns its geometry apart from the stored subscription
        ReferenceGeometry::Read(geometry) => geometry.clone(),
        ReferenceGeometry::Encoded { geometry_type, coordinates } => WrittenCoordinates::from_text(coordinates)
            .map_err(|source| GeoQError::Coordinates { source })?
            .read_as(*geometry_type)
            .map_err(|source| GeoQError::ReferenceGeometry { source })?,
    };
    let geoproperty = geo_q
        .geoproperty
        .as_ref()
        .map_or(expanded_properties::LOCATION.as_str(), AsRef::<str>::as_ref)
        .parse()
        .map_err(|source| GeoQError::Geoproperty { source })?;
    Ok(GeoQuery {
        georel: geo_q.georel.clone(), // clone: the compiled query owns its relation apart from the stored subscription
        geometry,
        geoproperty,
    })
}

/// Admit the `geoQ` a subscription create body or update fragment writes: a
/// reference geometry within `limits`, whose string-encoded coordinates, if
/// any, are a geometry.
///
/// Coordinates encoded as a string are counted from their text before
/// anything parses them; coordinates given as JSON were counted with the
/// request and are counted again here by their positions. An update fragment's
/// `null`, its request to remove the `geoQ` (clause 5.5.8), has nothing to
/// admit and is not passed in.
///
/// # Errors
///
/// Returns [`SubscriptionError::GeoQTooLarge`] for a reference geometry past
/// `limits`, and [`SubscriptionError::Validation`] carrying the [`GeoQError`]
/// for coordinates that are no geometry.
pub fn admit_geo_q(geo_q: &GeoQueryInput, limits: GeometryCensusLimits) -> Result<(), SubscriptionError> {
    geo_q.reference.admit(limits).map(drop).map_err(|refused| {
        if let QueryError::ReferenceGeometryTooComplex { over } = refused {
            SubscriptionError::GeoQTooLarge { over }
        } else {
            SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(GeoQError::Reference { source: Box::new(refused) }))
        }
    })
}

/// The spatial restriction a subscription's `geoQ` places on matching
/// (clauses 4.10 and 5.8.6).
#[derive(Debug)]
pub enum SubscriptionGeoFilter {
    /// The subscription carries no `geoQ`: the geo condition admits every
    /// entity.
    Unrestricted,
    /// The subscription's `geoQ`, compiled to the same [`GeoQuery`] a
    /// request-borne geo filter produces.
    Restricts(GeoQuery),
    /// The stored `geoQ` no longer compiles. It admits no entity: a defined
    /// geoquery the broker cannot evaluate is never widened into none.
    Unusable,
}

impl SubscriptionGeoFilter {
    /// The filter a stored `geoQ` places on matching. A `geoQ` that no longer
    /// compiles is logged, naming the subscription, and becomes
    /// [`SubscriptionGeoFilter::Unusable`].
    #[must_use]
    pub fn from_stored(geo_q: Option<&GeoQueryInput>, subscription: &str) -> SubscriptionGeoFilter {
        match geo_q.map(compile_geo_q) {
            None => SubscriptionGeoFilter::Unrestricted,
            Some(Ok(query)) => SubscriptionGeoFilter::Restricts(query),
            Some(Err(error)) => {
                warn!(subscription, %error, "stored subscription geoQ does not compile; it matches no entity until the subscription is updated");
                SubscriptionGeoFilter::Unusable
            }
        }
    }

    /// The compiled geoquery, when the filter restricts by one.
    #[must_use]
    pub const fn restriction(&self) -> Option<&GeoQuery> {
        match self {
            SubscriptionGeoFilter::Restricts(query) => Some(query),
            SubscriptionGeoFilter::Unrestricted | SubscriptionGeoFilter::Unusable => None,
        }
    }

    /// Whether the geo condition admits an entity, given whether it lies where
    /// the restriction asks when there is one.
    pub fn admits(&self, restriction_met: impl FnOnce(&GeoQuery) -> bool) -> bool {
        match self {
            SubscriptionGeoFilter::Unrestricted => true,
            SubscriptionGeoFilter::Restricts(query) => restriction_met(query),
            SubscriptionGeoFilter::Unusable => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        geo_filter::{GeoQError, SubscriptionGeoFilter, admit_geo_q, compile_geo_q},
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_query::geo::input::GeoQueryInput;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    const SUBSCRIPTION: &str = "urn:ngsi-ld:Subscription:test";

    /// Limits of `positions` positions and a hundred other values.
    fn limits(positions: usize) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit")),
            values: JsonValueLimit::new(NonZeroUsize::new(100).expect("a positive limit")),
        }
    }

    /// The coordinates of a `LineString` of `positions` positions.
    fn line(positions: usize) -> Value {
        (0..positions)
            .map(|index| json!([f64::from(u32::try_from(index).expect("a small fixture index")) * 0.01, 1.0]))
            .collect()
    }

    /// The `geoQ` a request writes, read as the request model reads it.
    fn geo_q(wire: Value) -> GeoQueryInput {
        serde_json::from_value(wire).expect("a readable geoQ")
    }

    /// A `geoQ` whose reference geometry is a `LineString` with `coordinates`.
    fn with_line(coordinates: &Value) -> GeoQueryInput {
        geo_q(json!({"georel": "intersects", "geometry": "LineString", "coordinates": coordinates}))
    }

    #[test]
    fn a_subscription_geo_q_past_the_position_budget_is_refused() {
        assert!(admit_geo_q(&with_line(&line(4)), limits(4)).is_ok());
        let refused = admit_geo_q(&with_line(&line(5)), limits(4)).expect_err("five positions exceed four");
        assert!(
            matches!(
                refused,
                SubscriptionError::GeoQTooLarge {
                    over: GeometryOverLimit::Positions { .. }
                }
            ),
            "{refused:?}"
        );
    }

    /// Table 5.2.13-1: coordinates encoded as a string are counted from their
    /// text, and refused before they are parsed.
    #[test]
    fn string_encoded_coordinates_past_the_budget_are_refused_before_parsing() {
        assert!(admit_geo_q(&with_line(&Value::String(line(4).to_string())), limits(4)).is_ok());
        let refused = admit_geo_q(&with_line(&Value::String(line(5).to_string())), limits(4)).expect_err("five positions exceed four");
        assert!(
            matches!(
                refused,
                SubscriptionError::GeoQTooLarge {
                    over: GeometryOverLimit::Positions { .. }
                }
            ),
            "{refused:?}"
        );
        let empties = Value::String(Value::Array(vec![json!([]); 200]).to_string());
        let refused = admit_geo_q(&with_line(&empties), limits(4)).expect_err("two hundred lists exceed a hundred values");
        assert!(
            matches!(
                refused,
                SubscriptionError::GeoQTooLarge {
                    over: GeometryOverLimit::Values { .. }
                }
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn string_encoded_coordinates_that_are_no_geometry_are_refused_as_bad_request_data() {
        let refused = admit_geo_q(&with_line(&json!("[[0, 0], ")), limits(4)).expect_err("not JSON");
        assert!(
            matches!(
                refused,
                SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(GeoQError::Reference { .. }))
            ),
            "{refused:?}"
        );
        let refused = admit_geo_q(&with_line(&json!("[[0, 0]]")), limits(4)).expect_err("one position traces no line");
        assert!(
            matches!(
                refused,
                SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(GeoQError::Reference { .. }))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_complete_geo_q_compiles_with_location_as_the_default_geoproperty() {
        let compiled = compile_geo_q(&geo_q(json!({"georel": "within", "geometry": "Point", "coordinates": [13.4, 52.5]}))).expect("a complete geoQ compiles");
        pretty_assertions::assert_eq!(compiled.geoproperty.as_str(), expanded_properties::LOCATION.as_str());
    }

    #[test]
    fn coordinates_encoded_as_a_string_compile_like_an_array() {
        assert!(compile_geo_q(&geo_q(json!({"georel": "within", "geometry": "Point", "coordinates": "[13.4, 52.5]"}))).is_ok());
    }

    /// Table 5.2.13-1: each of the three members has cardinality 1, so a `geoQ`
    /// missing one is no `GeoQuery` and is refused when the body is read.
    #[test]
    fn each_mandatory_member_missing_is_refused_when_the_geo_q_is_read() {
        for (wire, missing) in [
            (json!({"geometry": "Point", "coordinates": [0, 0]}), "georel"),
            (json!({"georel": "near;maxDistance==2000", "coordinates": [0, 0]}), "geometry"),
            (json!({"georel": "within", "geometry": "Point"}), "coordinates"),
        ] {
            let error = serde_json::from_value::<GeoQueryInput>(wire.clone()).expect_err("an incomplete geoQ is no GeoQuery");
            assert!(error.to_string().contains(missing), "{wire}: got {error}");
        }
    }

    #[test]
    fn a_georel_outside_clause_4_10_is_refused_when_the_geo_q_is_read() {
        assert!(serde_json::from_value::<GeoQueryInput>(json!({"georel": "nearby", "geometry": "Point", "coordinates": [0, 0]})).is_err());
    }

    #[test]
    fn a_reference_position_off_the_axes_is_refused_with_its_detail() {
        let error = serde_json::from_value::<GeoQueryInput>(json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]}))
            .expect_err("latitude 100");
        assert!(error.to_string().contains("latitude 100 is outside [-90, 90]"), "{error}");
    }

    #[test]
    fn an_absent_geo_q_does_not_restrict() {
        assert!(matches!(
            SubscriptionGeoFilter::from_stored(None, SUBSCRIPTION),
            SubscriptionGeoFilter::Unrestricted
        ));
        assert!(SubscriptionGeoFilter::Unrestricted.admits(|_| false));
    }

    /// Clause 5.8.6: a defined geoquery the broker cannot evaluate admits no
    /// entity, rather than every entity as an absent one would.
    #[test]
    fn a_stored_geo_q_that_no_longer_compiles_admits_nothing() {
        let stored = geo_q(json!({"georel": "within", "geometry": "Point", "coordinates": "[1, "}));
        let filter = SubscriptionGeoFilter::from_stored(Some(&stored), SUBSCRIPTION);
        assert!(matches!(filter, SubscriptionGeoFilter::Unusable), "got {filter:?}");
        assert!(!filter.admits(|_| true));
        assert!(filter.restriction().is_none());
    }

    #[test]
    fn a_compiled_restriction_admits_what_the_geoquery_matches() {
        let stored = geo_q(json!({"georel": "within", "geometry": "Point", "coordinates": [1, 2]}));
        let filter = SubscriptionGeoFilter::from_stored(Some(&stored), SUBSCRIPTION);
        assert!(filter.restriction().is_some());
        assert!(filter.admits(|_| true));
        assert!(!filter.admits(|_| false));
    }
}
