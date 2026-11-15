//! The members IETF RFC 7946 describes for a geometry object.
//!
//! Every reader and writer of a geometry object names its members here, and a
//! member by any other name is a foreign member (section 6.1).

use std::num::NonZeroU8;
use strum::{EnumIter, EnumString, IntoStaticStr};

/// The fractional digits a stored geometry writes a coordinate back with as
/// `GeoJSON`, fifteen, which a coordinate member is read and compared at, so a
/// `q` over a `GeoProperty`'s `coordinates` compares a coordinate alike in
/// memory and in the store.
pub const COORDINATE_DECIMALS: u8 = 15;

/// The significant digits of a coordinate written in exponent notation: one
/// before the mantissa's point and [`COORDINATE_DECIMALS`] after it.
pub const COORDINATE_SIGNIFICANT_DIGITS: NonZeroU8 = NonZeroU8::MIN.saturating_add(COORDINATE_DECIMALS);

/// The magnitude at or below which a coordinate is written in exponent
/// notation.
const FIXED_POINT_FLOOR: f64 = 1e-8;

/// The magnitude at or above which a coordinate is written in exponent
/// notation.
const FIXED_POINT_CEILING: f64 = 1e15;

/// How a stored geometry writes one coordinate back as `GeoJSON`.
///
/// It rounds the coordinate's shortest round-trip decimal spelling, not the
/// exact value of the double, half to even: to [`COORDINATE_DECIMALS`]
/// fractional digits in fixed-point notation, or to
/// [`COORDINATE_SIGNIFICANT_DIGITS`] in exponent notation, and drops trailing
/// zeros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateNotation {
    /// Fixed-point, for a magnitude strictly between `1e-8` and `1e15`.
    FixedPoint,
    /// Exponent notation, for every other magnitude.
    Exponent,
}

impl CoordinateNotation {
    /// The notation `coordinate` is written back in.
    #[must_use]
    pub const fn of(coordinate: f64) -> CoordinateNotation {
        let magnitude = coordinate.abs();
        if magnitude > FIXED_POINT_FLOOR && magnitude < FIXED_POINT_CEILING {
            CoordinateNotation::FixedPoint
        } else {
            CoordinateNotation::Exponent
        }
    }
}

/// A member RFC 7946 describes for a geometry object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, EnumString, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum GeometryMember {
    /// `type`, the geometry type (section 3.1).
    Type,
    /// `coordinates`, the positions of every geometry but a collection
    /// (section 3.1.1).
    Coordinates,
    /// `bbox`, the bounding box (section 5).
    Bbox,
    /// `geometries`, the geometries of a `GeometryCollection` (section 3.1.8).
    Geometries,
}

impl GeometryMember {
    /// The member's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry_member::GeometryMember;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn every_member_is_named_as_rfc_7946_spells_it() {
        pretty_assertions::assert_eq!(
            GeometryMember::iter().map(GeometryMember::as_str).collect::<Vec<_>>(),
            ["type", "coordinates", "bbox", "geometries"]
        );
        for member in GeometryMember::iter() {
            pretty_assertions::assert_eq!(GeometryMember::from_str(member.as_str()), Ok(member));
        }
        assert!(GeometryMember::from_str("Coordinates").is_err());
    }
}
