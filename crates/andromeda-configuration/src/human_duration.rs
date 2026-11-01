//! A duration rendered and parsed in the human-readable `humantime` form.

use humantime_serde as humantime;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt::{self, Display, Formatter},
    time::Duration,
};

/// The units a duration is written in, largest first, each with the seconds
/// or nanoseconds it spans and its `humantime` spellings for one and for more.
///
/// Days are the largest: `humantime` would render a month as 30.44 days and a
/// year as 365.25, which no calendar month or year is, so a whole number of
/// days is written as days.
const SECOND_UNITS: [(u64, &str, &str); 4] = [(86_400, "day", "days"), (3_600, "h", "h"), (60, "m", "m"), (1, "s", "s")];

/// The sub-second units, largest first, with the nanoseconds each spans.
const NANOSECOND_UNITS: [(u32, &str); 3] = [(1_000_000, "ms"), (1_000, "us"), (1, "ns")];

/// A [`Duration`] whose wire form is the human-readable `humantime` spelling
/// (`"30s"`, `"1h"`, `"7days"`) rather than the `{ secs, nanos }` shape serde
/// derives for a bare `Duration`.
///
/// The representation lives on the type rather than in a field's
/// `#[serde(with = "humantime_serde")]`, so the template, the effective
/// configuration and the JSON Schema all see the same spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[schemars(
    with = "String",
    title = "duration",
    description = "A duration such as \"500ms\", \"30s\", \"15m\", \"1h 30m\" or \"7days\"."
)]
pub struct HumanDuration(Duration);

impl HumanDuration {
    /// Wrap a [`Duration`] for human-readable serialisation.
    #[must_use]
    pub const fn new(duration: Duration) -> HumanDuration {
        HumanDuration(duration)
    }

    /// Construct from a whole number of milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> HumanDuration {
        HumanDuration(Duration::from_millis(millis))
    }

    /// Construct from a whole number of seconds.
    #[must_use]
    pub const fn from_secs(secs: u64) -> HumanDuration {
        HumanDuration(Duration::from_secs(secs))
    }

    /// Construct from a whole number of days.
    ///
    /// Used by the temporal day-grained fields whose defaults are expressed in
    /// days (`chunk_interval`, `columnstore_after`, `retention_horizon`).
    #[must_use]
    pub const fn from_days(days: u64) -> HumanDuration {
        HumanDuration(Duration::from_secs(days * 86_400))
    }

    /// The wrapped [`Duration`].
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }

    /// Whether the duration is exactly zero, used by execution-limit validation.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

// Manual: the spelling is computed from the duration, unit by unit, which no
// derive expresses.
impl Display for HumanDuration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if self.0.is_zero() {
            return formatter.write_str("0s");
        }
        let mut seconds = self.0.as_secs();
        let mut nanos = self.0.subsec_nanos();
        let mut separator = "";
        for (span, one, many) in SECOND_UNITS {
            let count = seconds / span;
            seconds %= span;
            if count > 0 {
                write!(formatter, "{separator}{count}{}", if count == 1 { one } else { many })?;
                separator = " ";
            }
        }
        for (span, unit) in NANOSECOND_UNITS {
            let count = nanos / span;
            nanos %= span;
            if count > 0 {
                write!(formatter, "{separator}{count}{unit}")?;
                separator = " ";
            }
        }
        Ok(())
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<HumanDuration, D::Error>
    where
        D: Deserializer<'de>,
    {
        humantime::deserialize(deserializer).map(HumanDuration)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::human_duration::HumanDuration;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Serialize, Deserialize)]
    struct Holder {
        span: HumanDuration,
    }

    #[test]
    fn human_duration_serialises_as_a_humantime_string() {
        let holder = Holder {
            span: HumanDuration::from_secs(30),
        };
        let rendered = toml::to_string(&holder).expect("holder serialises");
        assert!(rendered.contains("span = \"30s\""), "{rendered}");
    }

    #[test]
    fn human_duration_parses_a_humantime_string() {
        let holder: Holder = toml::from_str("span = \"45s\"").expect("holder parses");
        pretty_assertions::assert_eq!(holder.span.duration(), Duration::from_secs(45));
    }

    #[test]
    fn from_millis_round_trips_as_milliseconds() {
        let holder = Holder {
            span: HumanDuration::from_millis(500),
        };
        let rendered = toml::to_string(&holder).expect("holder serialises");
        assert!(rendered.contains("span = \"500ms\""), "{rendered}");
    }

    /// A month and a year are no fixed number of days, so a whole number of
    /// days is written in days whatever its length.
    #[test]
    fn a_whole_number_of_days_is_written_in_days() {
        for (days, written) in [(1, "1day"), (7, "7days"), (30, "30days"), (180, "180days"), (365, "365days")] {
            pretty_assertions::assert_eq!(HumanDuration::from_days(days).to_string(), written);
        }
    }

    #[test]
    fn each_unit_down_to_the_nanosecond_is_written_and_reads_back() {
        let span = Duration::new(86_400 + 2 * 3_600 + 3 * 60 + 4, 5_006_007);
        let written = HumanDuration::new(span).to_string();
        pretty_assertions::assert_eq!(written, "1day 2h 3m 4s 5ms 6us 7ns");
        let holder: Holder = toml::from_str(&format!("span = \"{written}\"")).expect("the spelling parses");
        pretty_assertions::assert_eq!(holder.span.duration(), span);
    }

    #[test]
    fn zero_is_written_as_zero_seconds() {
        pretty_assertions::assert_eq!(HumanDuration::from_secs(0).to_string(), "0s");
    }

    #[test]
    fn from_days_is_a_whole_number_of_days() {
        pretty_assertions::assert_eq!(HumanDuration::from_days(7).duration().as_secs(), 604_800);
    }

    #[test]
    fn zero_is_reported_as_zero() {
        assert!(HumanDuration::from_secs(0).is_zero());
        assert!(!HumanDuration::from_secs(1).is_zero());
    }
}
