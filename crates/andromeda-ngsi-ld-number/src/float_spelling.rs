//! The canonical JSON spelling of a binary64 value.
//!
//! Some values leave the exact numeric domain on purpose: a geometry's
//! ordinates are held as `f64` for every spatial computation. Writing one back
//! as JSON uses the shortest decimal that reads back to the same double,
//! spelled the way [`JsonNumber`] renders that decimal, so a stored ordinate
//! and an exact number of the same value are spelled alike.

use crate::{JsonNumber, JsonNumberError, number_token::NumberToken};

/// The canonical JSON spelling of `value`: its shortest round-trip decimal in
/// the rendering [`JsonNumber`] gives that decimal (`1.0` is `1`, `0.04522`
/// stays `0.04522`).
///
/// The shortest plain decimal Rust prints is taken as it stands whenever it is
/// already the canonical spelling, which covers every ordinate of a real-world
/// geometry without building a decimal; anything else goes through
/// [`JsonNumber`].
///
/// # Errors
///
/// Returns [`JsonNumberError::NotFinite`] for a NaN or an infinity, which no
/// JSON number spells.
pub fn canonical_float_text(value: f64) -> Result<String, JsonNumberError> {
    let shortest = value.to_string();
    if NumberToken::parse(&shortest).is_some_and(|token| token.is_canonical()) {
        return Ok(shortest);
    }
    JsonNumber::try_from(value).map(|number| number.to_string())
}

#[cfg(test)]
mod tests {
    use crate::{JsonNumber, JsonNumberError, float_spelling::canonical_float_text};

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
}
