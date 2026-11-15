//! How a stored geometry writes one coordinate back as `GeoJSON`, which is how
//! a `q` over a `GeoProperty`'s `coordinates` reads it in every execution: the
//! store prints the geometry, and the in-memory evaluator rounds each
//! coordinate as that printer does.

use std::num::NonZeroU8;

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

#[cfg(test)]
mod tests {
    use crate::coordinate_notation::CoordinateNotation;

    /// The fixed-point range is open at both ends: `1e-8` and `1e15` are
    /// written in exponent notation, whatever their sign, and a magnitude
    /// just inside either bound in fixed point.
    #[test]
    fn the_fixed_point_range_is_open_at_both_ends() {
        for exponent in [1e-8, -1e-8, 1e15, -1e15, 0.0, 1e-9, 2e15] {
            assert_eq!(CoordinateNotation::of(exponent), CoordinateNotation::Exponent, "{exponent}");
        }
        for fixed in [1.000_000_1e-8, -1.000_000_1e-8, 999_999_999_999_999.0, 52.6, -74.006] {
            assert_eq!(CoordinateNotation::of(fixed), CoordinateNotation::FixedPoint, "{fixed}");
        }
    }
}
