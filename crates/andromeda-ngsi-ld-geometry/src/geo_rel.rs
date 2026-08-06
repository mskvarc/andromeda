//! The `georel` spatial relationship (ETSI GS CIM 009 clause 4.10).

use serde::{Deserialize, Serialize};
use std::{fmt, num::ParseFloatError, str::FromStr};
use thiserror::Error;

/// Failures parsing a `georel` expression.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum GeoRelParseError {
    /// The relationship token is not one clause 4.10 defines.
    #[error("unknown georel: {0}")]
    UnknownRel(String),
    /// A `maxDistance` / `minDistance` modifier held a value that is not a number.
    #[error("invalid georel distance: {value}")]
    InvalidDistance {
        /// The offending modifier value.
        value: String,
        /// Underlying float parse failure.
        #[source]
        source: ParseFloatError,
    },
    /// A `maxDistance` / `minDistance` modifier parsed to a non-finite number
    /// (`NaN` or an infinity). Clause 4.10 gives these no meaning, and left
    /// unchecked they reach PostGIS as bare identifiers rather than a distance,
    /// turning a malformed request into a statement that errors at execution.
    #[error("non-finite georel distance: {value}")]
    NonFiniteDistance {
        /// The offending modifier value.
        value: String,
    },
    /// A `maxDistance` / `minDistance` modifier was zero or negative. Clause
    /// 4.10's grammar is `distance equal PositiveNumber`, so a non-positive
    /// distance is not a `near` query at all.
    #[error("non-positive georel distance: {value}")]
    NonPositiveDistance {
        /// The offending modifier value.
        value: String,
    },
    /// A `near` relationship carried no `maxDistance` or `minDistance`. Clause
    /// 4.10 requires exactly one mandatory positive distance, so an unbounded
    /// `near` has no meaning and is not representable.
    #[error("georel 'near' requires a maxDistance or minDistance")]
    NearMissingDistance,
    /// A `near` relationship carried a modifier that is neither `maxDistance`
    /// nor `minDistance`. Silently dropping it (as an earlier reading did) let
    /// a typo such as `maxDistanc==500` parse as an unbounded `near`.
    #[error("unknown georel modifier: {0}")]
    UnknownModifier(String),
}

/// Geo-spatial relationship operators per NGSI-LD clause 4.10.
///
/// These correspond to the `georel` query parameter values defined in the spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum GeoRel {
    /// Matches entities whose geometry is entirely within the reference geometry.
    Within,
    /// Matches entities whose geometry contains the reference geometry.
    Contains,
    /// Matches entities whose geometry intersects the reference geometry.
    Intersects,
    /// Matches entities whose geometry is disjoint from the reference geometry.
    Disjoint,
    /// Matches entities whose geometry is topologically equal to the reference geometry.
    Equals,
    /// Matches entities whose geometry overlaps the reference geometry.
    Overlaps,
    /// Matches entities within a distance range from the reference geometry.
    /// At least one of `max_distance` or `min_distance` must be specified.
    Near {
        /// Maximum distance in meters. Entities farther than this are excluded.
        max_distance: Option<f64>,
        /// Minimum distance in meters. Entities closer than this are excluded.
        min_distance: Option<f64>,
    },
}

// Hand-written because the `near` variant renders its modifiers back into the
// same `;`-separated form clause 4.10 defines, which `strum::Display` cannot
// express for a struct variant.
impl fmt::Display for GeoRel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoRel::Within => f.write_str("within"),
            GeoRel::Contains => f.write_str("contains"),
            GeoRel::Intersects => f.write_str("intersects"),
            GeoRel::Disjoint => f.write_str("disjoint"),
            GeoRel::Equals => f.write_str("equals"),
            GeoRel::Overlaps => f.write_str("overlaps"),
            GeoRel::Near { max_distance, min_distance } => {
                f.write_str("near")?;
                if let Some(max) = max_distance {
                    write!(f, ";maxDistance=={max}")?;
                }
                if let Some(min) = min_distance {
                    write!(f, ";minDistance=={min}")?;
                }
                Ok(())
            }
        }
    }
}

// Hand-written for the same reason as `Display`: `strum::EnumString` matches a
// whole string against a variant name, and `near` is a prefix whose `;`-joined
// modifiers have to be parsed into the struct variant's fields.
impl FromStr for GeoRel {
    type Err = GeoRelParseError;

    fn from_str(value: &str) -> Result<GeoRel, GeoRelParseError> {
        let mut parts = value.split(';');
        let rel = parts.next().unwrap_or_default().trim();
        match rel {
            "within" => Ok(GeoRel::Within),
            "contains" => Ok(GeoRel::Contains),
            "intersects" => Ok(GeoRel::Intersects),
            "disjoint" => Ok(GeoRel::Disjoint),
            "equals" => Ok(GeoRel::Equals),
            "overlaps" => Ok(GeoRel::Overlaps),
            "near" => {
                let mut max_distance = None;
                let mut min_distance = None;
                for part in parts {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    if let Some(raw) = part.strip_prefix("maxDistance==") {
                        max_distance = Some(parse_distance(raw)?);
                    } else if let Some(raw) = part.strip_prefix("minDistance==") {
                        min_distance = Some(parse_distance(raw)?);
                    } else {
                        // A malformed modifier is a rejected request, never a
                        // silently unbounded `near`.
                        return Err(GeoRelParseError::UnknownModifier(part.to_string()));
                    }
                }
                if max_distance.is_none() && min_distance.is_none() {
                    return Err(GeoRelParseError::NearMissingDistance);
                }
                Ok(GeoRel::Near { max_distance, min_distance })
            }
            other => Err(GeoRelParseError::UnknownRel(other.to_string())),
        }
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for GeoRel {
    type Error = GeoRelParseError;

    fn try_from(value: String) -> Result<GeoRel, GeoRelParseError> {
        value.parse()
    }
}

// The wire form is produced by `Display`; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl From<GeoRel> for String {
    fn from(value: GeoRel) -> String {
        value.to_string()
    }
}

/// Parse one `maxDistance` / `minDistance` modifier value, in metres.
///
/// Rust's `f64` parser accepts `NaN`, `inf` and `infinity`; those are rejected
/// here so a non-finite distance never reaches the statement builder. Clause
/// 4.10's grammar requires a `PositiveNumber`, so zero and negatives are
/// rejected too.
fn parse_distance(raw: &str) -> Result<f64, GeoRelParseError> {
    let parsed = raw.parse::<f64>().map_err(|source| GeoRelParseError::InvalidDistance {
        value: raw.to_string(),
        source,
    })?;
    if !parsed.is_finite() {
        return Err(GeoRelParseError::NonFiniteDistance { value: raw.to_string() });
    }
    if parsed <= 0.0 {
        return Err(GeoRelParseError::NonPositiveDistance { value: raw.to_string() });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use crate::geo_rel::{GeoRel, GeoRelParseError};

    #[test]
    fn every_simple_georel_round_trips_through_its_spec_token() {
        for (token, expected) in [
            ("within", GeoRel::Within),
            ("contains", GeoRel::Contains),
            ("intersects", GeoRel::Intersects),
            ("disjoint", GeoRel::Disjoint),
            ("equals", GeoRel::Equals),
            ("overlaps", GeoRel::Overlaps),
        ] {
            pretty_assertions::assert_eq!(token.parse::<GeoRel>().expect(token), expected);
            pretty_assertions::assert_eq!(expected.to_string(), token);
        }
    }

    #[test]
    fn near_carries_its_distance_modifiers_both_ways() {
        let parsed: GeoRel = "near;minDistance==50;maxDistance==500".parse().expect("near");
        pretty_assertions::assert_eq!(
            parsed,
            GeoRel::Near {
                max_distance: Some(500.0),
                min_distance: Some(50.0)
            }
        );
        pretty_assertions::assert_eq!(parsed.to_string(), "near;maxDistance==500;minDistance==50");
    }

    #[test]
    fn an_unknown_relationship_is_a_typed_error() {
        pretty_assertions::assert_eq!(
            "beside".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::UnknownRel("beside".to_string())
        );
    }

    #[test]
    fn a_non_numeric_distance_is_a_typed_error() {
        assert!(matches!(
            "near;maxDistance==far".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::InvalidDistance { .. }
        ));
    }

    #[test]
    fn a_non_finite_distance_is_rejected() {
        // Rust's f64 parser accepts these spellings; clause 4.10 gives them no
        // meaning, so they are rejected before reaching the statement builder.
        for raw in ["NaN", "inf", "infinity", "-inf"] {
            assert!(
                matches!(
                    format!("near;maxDistance=={raw}").parse::<GeoRel>().expect_err("must fail"),
                    GeoRelParseError::NonFiniteDistance { .. }
                ),
                "{raw} should be rejected as non-finite"
            );
        }
    }

    #[test]
    fn near_without_either_distance_is_rejected() {
        // Clause 4.10: `nearRel` requires one mandatory positive distance, so a
        // bare `near` is not a valid relationship.
        pretty_assertions::assert_eq!("near".parse::<GeoRel>().expect_err("must fail"), GeoRelParseError::NearMissingDistance);
    }

    #[test]
    fn a_non_positive_near_distance_is_rejected() {
        // Clause 4.10's grammar is `distance equal PositiveNumber`.
        for raw in ["0", "-5", "0.0"] {
            assert!(
                matches!(
                    format!("near;maxDistance=={raw}").parse::<GeoRel>().expect_err("must fail"),
                    GeoRelParseError::NonPositiveDistance { .. }
                ),
                "{raw} should be rejected as non-positive"
            );
        }
    }

    #[test]
    fn a_malformed_near_modifier_is_rejected_rather_than_dropped() {
        // A typo in the modifier key must fail, not silently parse as an
        // unbounded `near`.
        assert!(matches!(
            "near;maxDistanc==500".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::UnknownModifier(_)
        ));
    }

    #[test]
    fn georel_serialises_as_its_wire_token() {
        pretty_assertions::assert_eq!(serde_json::to_value(GeoRel::Within).expect("serialises"), serde_json::json!("within"));
        let decoded: GeoRel = serde_json::from_value(serde_json::json!("near;maxDistance==1000")).expect("deserialises");
        pretty_assertions::assert_eq!(
            decoded,
            GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None
            }
        );
    }
}
