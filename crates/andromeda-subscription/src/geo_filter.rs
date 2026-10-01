//! A subscription's `geoQ` (ETSI GS CIM 009 v1.9.1 clause 5.2.13), read once
//! for both admission and matching.
//!
//! Table 5.2.13-1 gives `georel`, `geometry` and `coordinates` cardinality 1
//! and `geoproperty` 0..1, defaulting to `location`; clause 4.10 needs all
//! three "to fully specify an NGSI-LD Geoquery". One reading,
//! [`compile_geo_q`], serves both sides: a create or update whose `geoQ` it
//! refuses is `BadRequestData` (clauses 5.8.1.4 and 5.5.4), and the matcher
//! compiles the stored `geoQ` with it. A written `geoQ` is admitted by
//! [`admit_geo_q`], which takes the census of string-encoded coordinates
//! before they are parsed and compiles the `geoQ` once for both its validity
//! and its size. Reading them separately is how a
//! `geoQ` admission let through (one naming only `georel`) came to compile to
//! nothing at match time, which the matcher took for "no `geoQ`" and so
//! notified every entity.
//!
//! A stored `geoQ` that no longer compiles, one written before a rule it
//! breaks existed, restricts matching to nothing. Clause 5.8.6 includes the
//! entities that match the geoquery, and all of them only "if query, geoquery
//! and scopeQuery are all not defined"; a defined geoquery the broker cannot
//! evaluate admits no entity, and widening it into none would notify a
//! subscriber of everything.

use crate::{error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_ngsi_ld_geometry::{
    geo_rel::GeoRelParseError,
    geojson::GeometryError,
    geometry::Geometry,
    geometry_census::{GeometryCensusError, GeometryCensusLimits, text_coordinates_census},
    geometry_type::GeometryType,
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_ngsi_ld_query::geo::query::GeoQuery;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use strum::Display;
use thiserror::Error;
use tracing::warn;

/// A member Table 5.2.13-1 makes mandatory, named as the wire spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum GeoQMember {
    /// The geo-relationship of clause 4.10.
    Georel,
    /// The type of the reference geometry.
    Geometry,
    /// The coordinates of the reference geometry.
    Coordinates,
}

/// Why a `geoQ` is not a `GeoQuery` the broker can evaluate.
#[derive(Debug, Error)]
pub enum GeoQError {
    /// The `geoQ` was not a JSON object.
    #[error("geoQ must be a GeoQuery object")]
    NotAnObject,
    /// A member Table 5.2.13-1 makes mandatory was absent.
    #[error("geoQ is missing '{member}', which Table 5.2.13-1 requires")]
    MissingMember {
        /// The absent member.
        member: GeoQMember,
    },
    /// A member Table 5.2.13-1 types as a string was not one.
    #[error("geoQ {member} must be a string")]
    MemberNotString {
        /// The member of the wrong type.
        member: GeoQMember,
    },
    /// The `georel` was not a clause 4.10 geo-relationship.
    #[error("geoQ georel: {source}")]
    Georel {
        /// Why the relationship does not parse.
        #[source]
        source: GeoRelParseError,
    },
    /// The `geometry` named no `GeoJSON` geometry type.
    #[error("geoQ geometry: {source}")]
    GeometryType {
        /// The unknown geometry-type token.
        #[source]
        source: strum::ParseError,
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
/// Returns the first [`GeoQError`] the `geoQ` meets: not an object, a
/// mandatory member absent or mistyped, or a member whose value is no valid
/// geo-relationship, geometry type, reference geometry or attribute name.
pub fn compile_geo_q(geo_q: &Value) -> Result<GeoQuery, GeoQError> {
    let object = geo_q.as_object().ok_or(GeoQError::NotAnObject)?;
    let string_member = |member: GeoQMember, key: &str| match object.get(key) {
        Some(Value::String(value)) => Ok(value.as_str()),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => Err(GeoQError::MemberNotString { member }),
        None => Err(GeoQError::MissingMember { member }),
    };
    let georel = string_member(GeoQMember::Georel, "georel")?
        .parse()
        .map_err(|source| GeoQError::Georel { source })?;
    let geometry_type = string_member(GeoQMember::Geometry, "geometry")?
        .parse::<GeometryType>()
        .map_err(|source| GeoQError::GeometryType { source })?;
    let coordinates = match object.get("coordinates") {
        // Table 5.2.13-1: "For the sake of JSON-LD compatibility It can be
        // encoded as a string".
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw).map_err(|source| GeoQError::Coordinates { source })?,
        Some(other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_))) => other.clone(), // clone: the GeoJSON decoder consumes the value it decodes
        None => {
            return Err(GeoQError::MissingMember {
                member: GeoQMember::Coordinates,
            });
        }
    };
    let geometry = Geometry::from_type_and_coordinates(geometry_type, coordinates).map_err(|source| GeoQError::ReferenceGeometry { source })?;
    let geoproperty = object
        .get("geoproperty")
        .and_then(Value::as_str)
        .unwrap_or(expanded_properties::LOCATION.as_str())
        .parse()
        .map_err(|source| GeoQError::Geoproperty { source })?;
    Ok(GeoQuery { georel, geometry, geoproperty })
}

/// Admit the `geoQ` a subscription create body or update fragment writes:
/// a `GeoQuery` the broker can evaluate, whose reference geometry stays within
/// `limits` (clause 6.3.2: a written input larger than the broker processes
/// is "Request Entity too large").
///
/// Coordinates encoded as a string (Table 5.2.13-1) are counted from their
/// text before anything parses them; the `geoQ` is then compiled once, which
/// both refuses a malformed one and counts the positions of one given as an
/// array. A payload without a `geoQ`, or whose `geoQ` is `null` -- an update
/// fragment's request to remove it (clause 5.5.8) -- has nothing to admit.
///
/// # Errors
///
/// Returns [`SubscriptionError::GeoQTooLarge`] for a reference geometry past
/// `limits`, and [`SubscriptionError::Validation`] carrying the [`GeoQError`]
/// for a `geoQ` that is no `GeoQuery`.
pub fn admit_geo_q(payload: &Value, limits: GeometryCensusLimits) -> Result<(), SubscriptionError> {
    let Some(geo_q) = payload.get("geoQ").filter(|geo_q| !geo_q.is_null()) else {
        return Ok(());
    };
    if let Some(Value::String(coordinates)) = geo_q.get("coordinates") {
        match text_coordinates_census(coordinates, limits) {
            // Text that is no JSON is refused by the compilation below.
            Ok(_) | Err(GeometryCensusError::Syntax(_)) => {}
            Err(GeometryCensusError::OverLimit(over)) => return Err(SubscriptionError::GeoQTooLarge { over }),
        }
    }
    let geo_query = compile_geo_q(geo_q).map_err(|source| SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(source)))?;
    limits
        .admit_positions(geo_query.geometry.form().position_count())
        .map_err(|over| SubscriptionError::GeoQTooLarge { over })
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
    pub fn from_stored(geo_q: Option<&Value>, subscription: &str) -> SubscriptionGeoFilter {
        match geo_q.filter(|value| !value.is_null()).map(compile_geo_q) {
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
        geo_filter::{GeoQError, GeoQMember, SubscriptionGeoFilter, admit_geo_q, compile_geo_q},
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
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

    /// A subscription payload whose `geoQ` is a `LineString` with `coordinates`.
    fn with_line(coordinates: &Value) -> Value {
        json!({"geoQ": {"georel": "intersects", "geometry": "LineString", "coordinates": coordinates}})
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
        assert!(admit_geo_q(&json!({"geoQ": null}), limits(4)).is_ok());
        assert!(admit_geo_q(&json!({}), limits(4)).is_ok());
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
    fn a_malformed_geo_q_is_refused_as_bad_request_data() {
        let refused = admit_geo_q(&json!({"geoQ": {"georel": "near;maxDistance==2000"}}), limits(4)).expect_err("incomplete");
        assert!(
            matches!(
                refused,
                SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(GeoQError::MissingMember { .. }))
            ),
            "{refused:?}"
        );
        let refused = admit_geo_q(&with_line(&json!("[[0, 0], ")), limits(4)).expect_err("not JSON");
        assert!(
            matches!(
                refused,
                SubscriptionError::Validation(SubscriptionValidationReason::GeoQ(GeoQError::Coordinates { .. }))
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_complete_geo_q_compiles_with_location_as_the_default_geoproperty() {
        let compiled = compile_geo_q(&json!({"georel": "within", "geometry": "Point", "coordinates": [13.4, 52.5]})).expect("a complete geoQ compiles");
        pretty_assertions::assert_eq!(compiled.geoproperty.as_str(), expanded_properties::LOCATION.as_str());
    }

    #[test]
    fn coordinates_encoded_as_a_string_compile_like_an_array() {
        assert!(compile_geo_q(&json!({"georel": "within", "geometry": "Point", "coordinates": "[13.4, 52.5]"})).is_ok());
    }

    /// Table 5.2.13-1: each of the three members has cardinality 1.
    #[test]
    fn each_mandatory_member_missing_is_named() {
        for (geo_q, missing) in [
            (json!({"geometry": "Point", "coordinates": [0, 0]}), GeoQMember::Georel),
            (json!({"georel": "near;maxDistance==2000"}), GeoQMember::Geometry),
            (json!({"georel": "within", "geometry": "Point"}), GeoQMember::Coordinates),
        ] {
            let error = compile_geo_q(&geo_q).expect_err("an incomplete geoQ is no GeoQuery");
            assert!(
                matches!(error, GeoQError::MissingMember { member } if member == missing),
                "{geo_q}: got {error:?}"
            );
            assert!(error.to_string().contains(&format!("'{missing}'")), "{error}");
        }
    }

    #[test]
    fn a_georel_outside_clause_4_10_is_refused() {
        let error = compile_geo_q(&json!({"georel": "nearby", "geometry": "Point", "coordinates": [0, 0]})).expect_err("no such georel");
        assert!(matches!(error, GeoQError::Georel { .. }), "got {error:?}");
    }

    #[test]
    fn a_reference_position_off_the_axes_is_refused_with_its_detail() {
        let error = compile_geo_q(&json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]})).expect_err("latitude 100");
        assert!(error.to_string().contains("latitude 100 is outside [-90, 90]"), "{error}");
    }

    #[test]
    fn an_absent_or_null_geo_q_does_not_restrict() {
        assert!(matches!(
            SubscriptionGeoFilter::from_stored(None, SUBSCRIPTION),
            SubscriptionGeoFilter::Unrestricted
        ));
        assert!(matches!(
            SubscriptionGeoFilter::from_stored(Some(&json!(null)), SUBSCRIPTION),
            SubscriptionGeoFilter::Unrestricted
        ));
        assert!(SubscriptionGeoFilter::Unrestricted.admits(|_| false));
    }

    /// Clause 5.8.6: a defined geoquery the broker cannot evaluate admits no
    /// entity, rather than every entity as an absent one would.
    #[test]
    fn a_stored_geo_q_that_no_longer_compiles_admits_nothing() {
        let filter = SubscriptionGeoFilter::from_stored(Some(&json!({"georel": "near;maxDistance==2000"})), SUBSCRIPTION);
        assert!(matches!(filter, SubscriptionGeoFilter::Unusable), "got {filter:?}");
        assert!(!filter.admits(|_| true));
        assert!(filter.restriction().is_none());
    }

    #[test]
    fn a_compiled_restriction_admits_what_the_geoquery_matches() {
        let filter = SubscriptionGeoFilter::from_stored(Some(&json!({"georel": "within", "geometry": "Point", "coordinates": [1, 2]})), SUBSCRIPTION);
        assert!(filter.restriction().is_some());
        assert!(filter.admits(|_| true));
        assert!(!filter.admits(|_| false));
    }
}
