//! A duration rendered and parsed in the human-readable `humantime` form.

use humantime_serde as humantime;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// A [`Duration`] whose wire form is the human-readable `humantime` spelling
/// (`"30s"`, `"1h"`, `"7days"`) rather than the `{ secs, nanos }` shape serde
/// derives for a bare `Duration`.
///
/// The broker uses this newtype instead of `#[serde(with = "humantime_serde")]`
/// on a plain `Duration` field because the commented-configuration renderer
/// (`toml_comment::TomlComment`) serialises each field through the field type's
/// own `Serialize` impl and does not see a `with` attribute; a bare `Duration`
/// would therefore render as an inline table. Owning the representation on the
/// type keeps every rendering path -- the commented template, the effective-config
/// dump, and figment loading -- in agreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        humantime::serialize(&self.0, serializer)
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
