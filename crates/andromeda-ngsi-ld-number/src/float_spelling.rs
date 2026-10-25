//! The canonical JSON spelling of a binary64 value.
//!
//! Some values leave the exact numeric domain on purpose: a geometry's
//! ordinates are held as `f64` for every spatial computation. Writing one back
//! as JSON uses the shortest decimal that reads back to the same double,
//! spelled the way [`JsonNumber`] renders that decimal, so a stored ordinate
//! and an exact number of the same value are spelled alike.
//!
//! The shortest decimal is the one `zmij` writes, the formatter `serde_json`
//! writes every double with. When that decimal is already the canonical
//! spelling, which covers every ordinate of a real-world geometry that is not
//! a whole number, the double is handed to the serializer as it is and comes
//! out with those very digits, so writing a geometry of a million positions
//! allocates nothing per ordinate.

use crate::{JsonNumber, JsonNumberError, number_token::NumberToken};
use serde::{Serialize, Serializer, ser::Error};
use serde_json::Number;

/// The shortest round-trip decimal of a finite `value`, written into
/// `buffer`, when it is already the canonical spelling.
fn canonical_shortest(value: f64, buffer: &mut zmij::Buffer) -> Option<&str> {
    let shortest = buffer.format_finite(value);
    NumberToken::parse(shortest).is_some_and(|token| token.is_canonical()).then_some(shortest)
}

/// The canonical JSON spelling of `value`: its shortest round-trip decimal in
/// the rendering [`JsonNumber`] gives that decimal (`1.0` is `1`, `0.04522`
/// stays `0.04522`).
///
/// # Errors
///
/// Returns [`JsonNumberError::NotFinite`] for a NaN or an infinity, which no
/// JSON number spells.
pub fn canonical_float_text(value: f64) -> Result<String, JsonNumberError> {
    if value.is_finite()
        && let Some(shortest) = canonical_shortest(value, &mut zmij::Buffer::new())
    {
        return Ok(shortest.to_owned());
    }
    JsonNumber::try_from(value).map(|number| number.to_string())
}

/// A double written as the JSON number of its canonical spelling, the text
/// [`canonical_float_text`] gives; serializing a NaN or an infinity fails, as
/// no JSON number spells one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanonicalFloat(pub f64);

// Hand-written: the number is written by deciding which way its spelling
// takes, which no derive expresses.
impl Serialize for CanonicalFloat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let CanonicalFloat(value) = *self;
        if value.is_finite() && canonical_shortest(value, &mut zmij::Buffer::new()).is_some() {
            return serializer.serialize_f64(value);
        }
        let text = canonical_float_text(value).map_err(S::Error::custom)?;
        Number::from_string_unchecked(text).serialize(serializer)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        JsonNumber,
        JsonNumberError,
        float_spelling::{CanonicalFloat, canonical_float_text},
    };

    /// The JSON text `value` is written as.
    fn written(value: f64) -> Result<String, serde_json::Error> {
        serde_json::to_string(&CanonicalFloat(value))
    }

    #[test]
    fn a_whole_double_drops_its_fraction() {
        pretty_assertions::assert_eq!(canonical_float_text(1.0).ok(), Some("1".to_owned()));
        pretty_assertions::assert_eq!(canonical_float_text(-180.0).ok(), Some("-180".to_owned()));
    }

    #[test]
    fn a_fraction_keeps_its_shortest_round_trip_digits() {
        pretty_assertions::assert_eq!(canonical_float_text(0.04522).ok(), Some("0.04522".to_owned()));
        pretty_assertions::assert_eq!(canonical_float_text(179.845_135_6).ok(), Some("179.8451356".to_owned()));
        pretty_assertions::assert_eq!(canonical_float_text(-16.642_963_4).ok(), Some("-16.6429634".to_owned()));
    }

    #[test]
    fn negative_zero_is_spelled_as_zero() {
        pretty_assertions::assert_eq!(canonical_float_text(-0.0).ok(), Some("0".to_owned()));
    }

    #[test]
    fn values_past_the_plain_shortcut_take_the_exact_rendering() {
        for value in [100.0, 1e-7, 1.5e300, 0.001] {
            let expected = JsonNumber::try_from(value).map(|number| number.to_string()).ok();
            pretty_assertions::assert_eq!(canonical_float_text(value).ok(), expected, "{value}");
        }
    }

    #[test]
    fn every_spelling_reads_back_to_the_same_double() {
        for value in [0.1, 1e-7, 123_456.789, -45.000_001, 1.5e300, 5e-324] {
            let spelled = canonical_float_text(value).expect("finite");
            pretty_assertions::assert_eq!(spelled.parse::<f64>().ok(), Some(value), "{spelled}");
        }
    }

    #[test]
    fn a_non_finite_value_has_no_spelling() {
        assert!(matches!(canonical_float_text(f64::NAN), Err(JsonNumberError::NotFinite { .. })));
    }

    #[test]
    fn a_double_is_written_with_its_canonical_spelling() {
        for value in [
            1.0,
            -180.0,
            -0.0,
            0.0,
            13.4,
            179.845_135_6,
            -16.642_963_4,
            13.123_456_789_012_346,
            0.04522,
            0.001,
            1e-7,
            1.5e300,
            5e-324,
            100.0,
        ] {
            pretty_assertions::assert_eq!(written(value).ok(), canonical_float_text(value).ok(), "{value:e}");
        }
    }

    /// Ordinates across both CRS84 axes and a range of altitudes are written
    /// with the text the canonical spelling gives them, whichever way the
    /// writer takes.
    #[test]
    fn every_ordinate_of_a_sweep_is_written_with_its_canonical_spelling() {
        let longitudes = (0..20_000).map(|step| -180.0 + f64::from(step) * 0.018_000_37);
        let latitudes = (0..20_000).map(|step| -90.0 + f64::from(step) * 0.009_000_013_7);
        let altitudes = (0..20_000).map(|step| -11_000.0 + f64::from(step) * 1.234_567_891);
        for value in longitudes.chain(latitudes).chain(altitudes) {
            pretty_assertions::assert_eq!(written(value).ok(), canonical_float_text(value).ok(), "{value:e}");
        }
    }

    #[test]
    fn a_non_finite_double_is_not_written() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(written(value).is_err(), "{value}");
        }
    }
}
