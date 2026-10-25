//! On/off umbrella switch shared by the HTTP stack's optional features.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Shared on/off switch for an HTTP stack feature.
///
/// This is the configuration surface's single convention for a feature that is
/// either wholly on or wholly off -- the Server-Timing and CSRF umbrellas both
/// use it. A feature that instead exposes an enumerable set of independent
/// choices (the content codings) is modelled as a typed set listed as an array,
/// never as a family of boolean fields, and a genuine policy boolean that maps to
/// a boolean wire semantic (`allow_credentials`, `pass_through_unaccepted`) stays
/// a bare `bool`.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum HttpFeatureMode {
    /// Disable the feature and install no middleware for it.
    Disabled,
    /// Enable the feature.
    #[default]
    Enabled,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::feature_mode::HttpFeatureMode;

    #[test]
    fn feature_mode_round_trips_through_kebab_tokens() {
        for (mode, token) in [(HttpFeatureMode::Disabled, "disabled"), (HttpFeatureMode::Enabled, "enabled")] {
            pretty_assertions::assert_eq!(mode.to_string(), token);
            pretty_assertions::assert_eq!(token.parse::<HttpFeatureMode>().expect("mode parses"), mode);
        }
    }

    #[test]
    fn feature_mode_defaults_to_enabled() {
        pretty_assertions::assert_eq!(HttpFeatureMode::default(), HttpFeatureMode::Enabled);
    }
}
