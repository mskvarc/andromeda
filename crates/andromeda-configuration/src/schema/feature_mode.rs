//! The `mode` of a feature that is either wholly on or wholly off.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The `mode` of a feature that is either wholly on or wholly off.
///
/// A feature switch is always a named `mode`, never a boolean. A feature with
/// more than two states has a mode enum of its own, and an enumerable set of
/// independent choices (the content codings) is a typed set listed as an
/// array.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum FeatureMode {
    /// The feature is off.
    Disabled,
    /// Enable the feature.
    #[default]
    Enabled,
}

impl FeatureMode {
    /// Whether the feature is on.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, FeatureMode::Enabled)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::feature_mode::FeatureMode;

    #[test]
    fn feature_mode_round_trips_through_kebab_tokens() {
        for (mode, token) in [(FeatureMode::Disabled, "disabled"), (FeatureMode::Enabled, "enabled")] {
            pretty_assertions::assert_eq!(mode.to_string(), token);
            pretty_assertions::assert_eq!(token.parse::<FeatureMode>().expect("mode parses"), mode);
        }
    }

    #[test]
    fn feature_mode_defaults_to_enabled() {
        pretty_assertions::assert_eq!(FeatureMode::default(), FeatureMode::Enabled);
    }
}
