//! The ISO 8601 duration every NGSI-LD duration member carries.
//!
//! ETSI GS CIM 009 v1.9.1 types `refreshRate` (Table 5.2.9-1), `cacheDuration`
//! (Table 5.2.34-1), `contextSourceUptime` (Table 5.2.40-1), `snapshotLifetime`,
//! `entityMapLifetime` and `aggrPeriodDuration` (clause 4.5.19) as a string
//! representing a duration in ISO 8601 format, with no lower bound, so `PT0S`
//! is as valid as `PT1H`. ISO 8601 durations carry no sign and no exponent, so
//! a component is read only from digits and a decimal point or comma, and every
//! duration is written back in a form this reader accepts again: zero as `PT0S`.

use iso8601_duration::Duration as ParsedDuration;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::{fmt, str::FromStr, time::Duration};
use thiserror::Error;

/// Seconds in one day, for decomposing an elapsed time.
const SECONDS_PER_DAY: u64 = 86_400;
/// Seconds in one hour, for decomposing an elapsed time.
const SECONDS_PER_HOUR: u64 = 3_600;
/// Seconds in one minute, for decomposing an elapsed time.
const SECONDS_PER_MINUTE: u64 = 60;
/// Milliseconds in one second, for the fraction an elapsed time's seconds carry.
const MILLISECONDS_PER_SECOND: f32 = 1_000.0;

/// An ISO 8601 duration with finite, non-negative components.
///
/// Year and month components keep their calendar meaning: they only have a
/// length once anchored to an instant, so [`Iso8601Duration::to_std`] answers
/// only for a duration of days and below.
#[derive(Debug, Clone, Copy, PartialEq, SerializeDisplay, DeserializeFromStr)]
pub struct Iso8601Duration(ParsedDuration);

/// Why a text cannot be an [`Iso8601Duration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Iso8601DurationError {
    /// The text does not follow the ISO 8601 duration grammar.
    #[error("not an ISO 8601 duration at position {position}")]
    Malformed {
        /// Zero-based byte offset of the first character the grammar rejected.
        position: usize,
    },
    /// A component has more digits than a finite value holds.
    #[error("an ISO 8601 duration component is too large to hold")]
    ComponentOutOfRange,
}

impl Iso8601Duration {
    /// An elapsed time as days, hours, minutes and seconds, the seconds
    /// carrying the millisecond fraction.
    ///
    /// Hours, minutes and whole seconds are bounded by their moduli and held
    /// exactly. The day count is unbounded: it is exact up to 2^24 days, over
    /// 45 000 years, and rounded to the nearest `f32` above that.
    #[must_use]
    pub fn from_std(elapsed: Duration) -> Iso8601Duration {
        let whole_seconds = elapsed.as_secs();
        let days = whole_seconds / SECONDS_PER_DAY;
        let hours = (whole_seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR;
        let minutes = (whole_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
        let seconds = whole_seconds % SECONDS_PER_MINUTE;
        let milliseconds = u64::from(elapsed.subsec_millis());
        let seconds_with_fraction = whole_component(seconds) + whole_component(milliseconds) / MILLISECONDS_PER_SECOND;
        Iso8601Duration(ParsedDuration::new(
            0.0,
            0.0,
            whole_component(days),
            whole_component(hours),
            whole_component(minutes),
            seconds_with_fraction,
        ))
    }

    /// The years component.
    #[must_use]
    pub const fn years(&self) -> f32 {
        self.0.year
    }

    /// The months component.
    #[must_use]
    pub const fn months(&self) -> f32 {
        self.0.month
    }

    /// The days component, a week designator already counted as seven days.
    #[must_use]
    pub const fn days(&self) -> f32 {
        self.0.day
    }

    /// The hours component.
    #[must_use]
    pub const fn hours(&self) -> f32 {
        self.0.hour
    }

    /// The minutes component.
    #[must_use]
    pub const fn minutes(&self) -> f32 {
        self.0.minute
    }

    /// The seconds component, fraction included.
    #[must_use]
    pub const fn seconds(&self) -> f32 {
        self.0.second
    }

    /// Whether every component is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.components().iter().all(|component| *component == 0.0)
    }

    /// The fixed length of a duration of days and below.
    ///
    /// [`None`] when the duration has a year or month component, whose length
    /// depends on the instant it starts from, or when it exceeds what a
    /// standard-library duration holds.
    #[must_use]
    pub fn to_std(&self) -> Option<Duration> {
        if self.0.year != 0.0 || self.0.month != 0.0 {
            return None;
        }
        let seconds = f64::from(self.0.day) * 86_400.0 + f64::from(self.0.hour) * 3_600.0 + f64::from(self.0.minute) * 60.0 + f64::from(self.0.second);
        Duration::try_from_secs_f64(seconds).ok()
    }

    /// The six components, largest unit first.
    const fn components(&self) -> [f32; 6] {
        [self.0.year, self.0.month, self.0.day, self.0.hour, self.0.minute, self.0.second]
    }
}

/// One whole-number component of an elapsed time as the `f32` a duration holds.
///
/// Folded in from its 16-bit digits, each of which `f32` represents exactly, so
/// the count is exact while it fits the 24-bit significand.
fn whole_component(value: u64) -> f32 {
    let bytes = value.to_be_bytes();
    let (digits, _) = bytes.as_chunks::<2>();
    digits
        .iter()
        .fold(0.0, |held: f32, digit| held.mul_add(65_536.0, f32::from(u16::from_be_bytes(*digit))))
}

/// Whether `character` can appear in an ISO 8601 duration: a digit, a decimal
/// mark, or a designator.
const fn is_duration_character(character: char) -> bool {
    matches!(character, '0'..='9' | '.' | ',' | 'P' | 'Y' | 'M' | 'W' | 'D' | 'T' | 'H' | 'S')
}

impl FromStr for Iso8601Duration {
    type Err = Iso8601DurationError;

    fn from_str(text: &str) -> Result<Iso8601Duration, Iso8601DurationError> {
        // The upstream component reader accepts a sign, an exponent and the
        // words `inf` and `nan`, none of which ISO 8601 allows.
        if let Some(position) = text.find(|character| !is_duration_character(character)) {
            return Err(Iso8601DurationError::Malformed { position });
        }
        let parsed = ParsedDuration::parse(text).map_err(|error| Iso8601DurationError::Malformed { position: error.position })?;
        let duration = Iso8601Duration(parsed);
        if duration.components().iter().all(|component| component.is_finite()) {
            Ok(duration)
        } else {
            Err(Iso8601DurationError::ComponentOutOfRange)
        }
    }
}

// The upstream `Display` writes a zero duration as a bare `P`, which no ISO 8601
// reader accepts, its own included; `derive_more::Display` cannot express that
// one exception.
impl fmt::Display for Iso8601Duration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            formatter.write_str("PT0S")
        } else {
            fmt::Display::fmt(&self.0, formatter)
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::duration::{Iso8601Duration, Iso8601DurationError};
    use std::time::Duration;

    fn parsed(text: &str) -> Iso8601Duration {
        text.parse().expect("an ISO 8601 duration")
    }

    #[test]
    fn a_zero_duration_parses_writes_back_as_pt0s_and_reads_back_equal() {
        let zero = parsed("PT0S");
        assert!(zero.is_zero());
        pretty_assertions::assert_eq!(zero.to_string(), "PT0S");
        pretty_assertions::assert_eq!(parsed(&zero.to_string()), zero);
        pretty_assertions::assert_eq!(zero.to_std(), Some(Duration::ZERO));
    }

    #[test]
    fn every_zero_spelling_writes_back_as_pt0s() {
        for text in ["P0D", "P0Y", "PT0H0M0S", "P0W", "PT0.0S"] {
            pretty_assertions::assert_eq!(parsed(text).to_string(), "PT0S", "{text}");
        }
    }

    #[test]
    fn a_non_zero_duration_round_trips_unchanged() {
        for text in ["PT1H30M", "P1Y2M10DT2H30M", "P0.5M", "PT0.25S"] {
            pretty_assertions::assert_eq!(parsed(text).to_string(), text);
        }
    }

    #[test]
    fn a_bare_designator_and_an_empty_text_are_refused() {
        pretty_assertions::assert_eq!("P".parse::<Iso8601Duration>(), Err(Iso8601DurationError::Malformed { position: 1 }));
        pretty_assertions::assert_eq!("".parse::<Iso8601Duration>(), Err(Iso8601DurationError::Malformed { position: 0 }));
        assert!("PT".parse::<Iso8601Duration>().is_err());
    }

    /// ISO 8601 durations are unsigned and written in plain decimals.
    #[test]
    fn a_sign_an_exponent_and_a_non_number_are_refused_where_they_stand() {
        for (text, position) in [("PT-5S", 2), ("P+1D", 1), ("P1e3D", 2), ("PTinfS", 2), ("PTNaNS", 2)] {
            pretty_assertions::assert_eq!(text.parse::<Iso8601Duration>(), Err(Iso8601DurationError::Malformed { position }), "{text}");
        }
    }

    #[test]
    fn a_component_beyond_a_finite_value_is_refused() {
        let text = format!("P{}D", "9".repeat(50));
        pretty_assertions::assert_eq!(text.parse::<Iso8601Duration>(), Err(Iso8601DurationError::ComponentOutOfRange));
    }

    #[test]
    fn a_week_is_held_as_seven_days() {
        pretty_assertions::assert_eq!(parsed("P2W").to_string(), "P14D");
    }

    #[test]
    fn only_a_duration_of_days_and_below_has_a_fixed_length() {
        pretty_assertions::assert_eq!(parsed("P1DT1H1M1.5S").to_std(), Some(Duration::from_millis(90_061_500)));
        pretty_assertions::assert_eq!(parsed("P1M").to_std(), None);
        pretty_assertions::assert_eq!(parsed("P1Y").to_std(), None);
    }

    #[test]
    fn an_elapsed_time_is_held_as_days_hours_minutes_and_fractional_seconds() {
        let elapsed = Duration::from_millis(((2 * 86_400 + 3 * 3_600 + 4 * 60 + 5) * 1_000) + 250);
        pretty_assertions::assert_eq!(Iso8601Duration::from_std(elapsed).to_string(), "P2DT3H4M5.25S");
        pretty_assertions::assert_eq!(Iso8601Duration::from_std(Duration::ZERO).to_string(), "PT0S");
    }

    #[test]
    fn a_day_count_beyond_sixteen_bits_is_held_exactly() {
        let elapsed = Duration::from_hours(16_777_216 * 24);
        pretty_assertions::assert_eq!(Iso8601Duration::from_std(elapsed).to_string(), "P16777216D");
    }

    #[test]
    fn serde_writes_the_text_and_refuses_what_the_grammar_refuses() {
        let read: Iso8601Duration = serde_json::from_str("\"PT0S\"").expect("a zero duration deserializes");
        pretty_assertions::assert_eq!(serde_json::to_string(&read).expect("serializes"), "\"PT0S\"");
        assert!(serde_json::from_str::<Iso8601Duration>("\"P\"").is_err());
        assert!(serde_json::from_str::<Iso8601Duration>("\"\"").is_err());
        assert!(serde_json::from_str::<Iso8601Duration>("\"5 minutes\"").is_err());
    }
}
