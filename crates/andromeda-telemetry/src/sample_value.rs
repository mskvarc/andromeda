//! The floating-point value one recorded sample carries.

use metrics::IntoF64;

/// Scale of the upper 32 bits of a count once recombined into an `f64`.
const HIGH_HALF_SCALE: f64 = 4_294_967_296.0;

/// The value one recorded sample carries for a counted quantity.
///
/// A Prometheus sample is an `f64`, which represents every integer below 2^53
/// exactly. A count is converted through its two 32-bit halves, each of which
/// widens losslessly, so no figure this broker reports is rounded on the way
/// in: 2^53 is eight pebibytes of reclaimed storage, and no pool, queue or
/// candidate set comes near it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SampleValue(f64);

impl From<u64> for SampleValue {
    fn from(count: u64) -> SampleValue {
        let octets = count.to_le_bytes();
        let low = u32::from_le_bytes([octets[0], octets[1], octets[2], octets[3]]);
        let high = u32::from_le_bytes([octets[4], octets[5], octets[6], octets[7]]);
        SampleValue(f64::from(high) * HIGH_HALF_SCALE + f64::from(low))
    }
}

impl From<usize> for SampleValue {
    fn from(count: usize) -> SampleValue {
        // `usize` is at most 64 bits wide on every supported target, so the
        // ceiling is unreachable; it exists because the widening is fallible in
        // the type system, not because a count can outgrow a `u64`.
        SampleValue::from(u64::try_from(count).unwrap_or(u64::MAX))
    }
}

// The metrics facade takes a sample by trait and offers no derive for it, so
// the accessor is spelled out here.
impl IntoF64 for SampleValue {
    fn into_f64(self) -> f64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::sample_value::SampleValue;
    use metrics::IntoF64;

    #[test]
    fn a_count_within_the_u32_range_converts_exactly() {
        pretty_assertions::assert_eq!(SampleValue::from(4_294_967_295_u64).into_f64().to_bits(), 4_294_967_295.0_f64.to_bits());
    }

    #[test]
    fn a_count_spanning_both_halves_converts_exactly() {
        pretty_assertions::assert_eq!(SampleValue::from(78_187_493_530_u64).into_f64().to_bits(), 78_187_493_530.0_f64.to_bits());
    }

    #[test]
    fn a_pointer_sized_count_converts_exactly() {
        pretty_assertions::assert_eq!(SampleValue::from(3_usize).into_f64().to_bits(), 3.0_f64.to_bits());
    }

    #[test]
    fn a_zero_count_converts_to_zero() {
        pretty_assertions::assert_eq!(SampleValue::from(0_u64).into_f64().to_bits(), 0.0_f64.to_bits());
    }
}
