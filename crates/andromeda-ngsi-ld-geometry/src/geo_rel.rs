//! The `georel` spatial relationship (ETSI GS CIM 009 clause 4.10).

use crate::positive_distance::{PositiveDistance, PositiveDistanceParseError};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Failures parsing a `georel` expression.
#[derive(Debug, Error)]
pub enum GeoRelParseError {
    /// The relationship token is not one clause 4.10 defines.
    #[error("unknown georel")]
    UnknownRel,
    /// A `maxDistance` / `minDistance` modifier held a value that is not a valid
    /// clause 4.10 `PositiveNumber`.
    #[error(transparent)]
    InvalidDistance(#[from] PositiveDistanceParseError),
    /// A `near` relationship carried no `maxDistance` or `minDistance`. Clause
    /// 4.10 requires exactly one mandatory positive distance, so an unbounded
    /// `near` has no meaning and is not representable.
    #[error("georel 'near' requires a maxDistance or minDistance")]
    NearMissingDistance,
    /// A `near` relationship carried a modifier that is neither `maxDistance`
    /// nor `minDistance`, written exactly as clause 4.10 spells it. Dropping it
    /// would let a typo such as `maxDistanc==500` parse as an unbounded `near`.
    #[error("unknown georel modifier")]
    UnknownModifier,
    /// A relationship other than `near` carried a modifier. Clause 4.10 gives
    /// `withinRel`, `containsRel` and the other relations none.
    #[error("only georel 'near' takes a modifier")]
    UnexpectedModifier,
    /// A `near` relationship named the same distance modifier twice.
    #[error("georel 'near' names a distance modifier more than once")]
    RepeatedDistance,
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
    ///
    /// At least one of `max_distance` or `min_distance` is present. Clause 4.10
    /// admits one; both together are the distance band, which only the
    /// `near_distance_band` extension admits.
    Near {
        /// Maximum distance in meters. Entities farther than this are excluded.
        max_distance: Option<PositiveDistance>,
        /// Minimum distance in meters. Entities closer than this are excluded.
        min_distance: Option<PositiveDistance>,
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
        let (rel, modifiers) = value.split_once(';').map_or((value, None), |(rel, modifiers)| (rel, Some(modifiers)));
        let simple = |georel: GeoRel| modifiers.map_or(Ok(georel), |_| Err(GeoRelParseError::UnexpectedModifier));
        match rel {
            "within" => simple(GeoRel::Within),
            "contains" => simple(GeoRel::Contains),
            "intersects" => simple(GeoRel::Intersects),
            "disjoint" => simple(GeoRel::Disjoint),
            "equals" => simple(GeoRel::Equals),
            "overlaps" => simple(GeoRel::Overlaps),
            "near" => near(modifiers),
            _ => Err(GeoRelParseError::UnknownRel),
        }
    }
}

/// The `near` relationship its `;`-separated distance modifiers make.
///
/// Clause 4.10 writes the modifiers with no whitespace or empty segment, so
/// either is an unknown modifier rather than something to skip.
fn near(modifiers: Option<&str>) -> Result<GeoRel, GeoRelParseError> {
    let mut max_distance = None;
    let mut min_distance = None;
    for modifier in modifiers.into_iter().flat_map(|modifiers| modifiers.split(';')) {
        let (slot, raw) = if let Some(raw) = modifier.strip_prefix("maxDistance==") {
            (&mut max_distance, raw)
        } else if let Some(raw) = modifier.strip_prefix("minDistance==") {
            (&mut min_distance, raw)
        } else {
            return Err(GeoRelParseError::UnknownModifier);
        };
        if slot.replace(PositiveDistance::from_str(raw)?).is_some() {
            return Err(GeoRelParseError::RepeatedDistance);
        }
    }
    if max_distance.is_none() && min_distance.is_none() {
        return Err(GeoRelParseError::NearMissingDistance);
    }
    Ok(GeoRel::Near { max_distance, min_distance })
}

impl GeoRel {
    /// Whether this is a `near` bounded from both sides, the distance band
    /// clause 4.10 does not define.
    #[must_use]
    pub const fn is_distance_band(&self) -> bool {
        match self {
            GeoRel::Near {
                max_distance: Some(_),
                min_distance: Some(_),
            } => true,
            GeoRel::Near { .. } | GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Disjoint | GeoRel::Equals | GeoRel::Overlaps => false,
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

#[cfg(test)]
mod tests {
    use crate::{
        geo_rel::{GeoRel, GeoRelParseError},
        positive_distance::{PositiveDistance, PositiveDistanceParseError},
    };
    use std::str::FromStr;

    fn metres(raw: &str) -> PositiveDistance {
        PositiveDistance::from_str(raw).expect(raw)
    }

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
                max_distance: Some(metres("500")),
                min_distance: Some(metres("50"))
            }
        );
        pretty_assertions::assert_eq!(parsed.to_string(), "near;maxDistance==500;minDistance==50");
    }

    #[test]
    fn an_unknown_relationship_is_a_typed_error() {
        assert!(matches!("beside".parse::<GeoRel>().expect_err("must fail"), GeoRelParseError::UnknownRel));
    }

    #[test]
    fn a_non_numeric_distance_is_a_typed_error() {
        assert!(matches!(
            "near;maxDistance==far".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::InvalidDistance(PositiveDistanceParseError::InvalidSyntax(_))
        ));
    }

    #[test]
    fn a_non_finite_distance_is_rejected() {
        // These spellings are not RFC 8259 numbers, so the clause 4.10
        // `PositiveNumber` grammar rejects them before they become a distance.
        for raw in ["NaN", "inf", "infinity", "-inf"] {
            assert!(
                matches!(
                    format!("near;maxDistance=={raw}").parse::<GeoRel>().expect_err("must fail"),
                    GeoRelParseError::InvalidDistance(_)
                ),
                "{raw} should be rejected"
            );
        }
    }

    #[test]
    fn a_distance_outside_the_positive_number_grammar_is_rejected() {
        // Clause 4.10 spells the distance as a `PositiveNumber`, an RFC 8259
        // JSON number; `f64`'s own laxer parser is not the arbiter here.
        for raw in ["+500", ".5", "5.", "007", "1,000"] {
            assert!(
                matches!(
                    format!("near;maxDistance=={raw}").parse::<GeoRel>().expect_err("must fail"),
                    GeoRelParseError::InvalidDistance(PositiveDistanceParseError::InvalidSyntax(_))
                ),
                "{raw} should be rejected as invalid syntax"
            );
        }
    }

    #[test]
    fn near_without_either_distance_is_rejected() {
        // Clause 4.10: `nearRel` requires one mandatory positive distance, so a
        // bare `near` is not a valid relationship.
        assert!(matches!(
            "near".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::NearMissingDistance
        ));
    }

    #[test]
    fn a_non_positive_near_distance_is_rejected() {
        // Clause 4.10's grammar is `distance equal PositiveNumber`.
        for raw in ["0", "-5", "0.0"] {
            assert!(
                matches!(
                    format!("near;maxDistance=={raw}").parse::<GeoRel>().expect_err("must fail"),
                    GeoRelParseError::InvalidDistance(PositiveDistanceParseError::NonPositive(_))
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
            GeoRelParseError::UnknownModifier
        ));
    }

    #[test]
    fn a_modifier_on_a_relation_other_than_near_is_rejected() {
        for raw in ["within;maxDistance==5", "disjoint;minDistance==5", "equals;"] {
            assert!(
                matches!(raw.parse::<GeoRel>().expect_err("must fail"), GeoRelParseError::UnexpectedModifier),
                "{raw} should be rejected"
            );
        }
    }

    #[test]
    fn a_repeated_distance_modifier_is_rejected() {
        for raw in ["near;maxDistance==5;maxDistance==9", "near;minDistance==5;minDistance==9"] {
            assert!(
                matches!(raw.parse::<GeoRel>().expect_err("must fail"), GeoRelParseError::RepeatedDistance),
                "{raw} should be rejected"
            );
        }
    }

    #[test]
    fn whitespace_or_an_empty_segment_in_near_is_an_unknown_modifier() {
        for raw in ["near; maxDistance==5", "near;maxDistance==5;", "near;;maxDistance==5", "near;"] {
            assert!(
                matches!(raw.parse::<GeoRel>().expect_err("must fail"), GeoRelParseError::UnknownModifier),
                "{raw} should be rejected"
            );
        }
        assert!(matches!(
            " near;maxDistance==5".parse::<GeoRel>().expect_err("must fail"),
            GeoRelParseError::UnknownRel
        ));
    }

    #[test]
    fn only_a_near_with_both_distances_is_a_distance_band() {
        let band: GeoRel = "near;maxDistance==500;minDistance==50".parse().expect("band");
        assert!(band.is_distance_band());
        for raw in ["near;maxDistance==500", "near;minDistance==50", "within"] {
            assert!(!raw.parse::<GeoRel>().expect(raw).is_distance_band(), "{raw}");
        }
    }

    #[test]
    fn georel_serialises_as_its_wire_token() {
        pretty_assertions::assert_eq!(serde_json::to_value(GeoRel::Within).expect("serialises"), serde_json::json!("within"));
        let decoded: GeoRel = serde_json::from_value(serde_json::json!("near;maxDistance==1000")).expect("deserialises");
        pretty_assertions::assert_eq!(
            decoded,
            GeoRel::Near {
                max_distance: Some(metres("1000")),
                min_distance: None
            }
        );
    }
}
