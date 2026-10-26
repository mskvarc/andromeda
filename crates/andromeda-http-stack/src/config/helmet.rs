//! Security-header (helmet) policy.

use crate::config::feature_mode::HttpFeatureMode;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// Security-header configuration.
///
/// The broker installs a default set of security response headers
/// (`Content-Security-Policy`, `X-Content-Type-Options`, the cross-origin
/// isolation trio, and the rest of the `axum-helmet` defaults) on the API
/// subtree. `Strict-Transport-Security` is part of the policy too, but a response
/// carries it only when the request arrived over a TLS listener. Enabled by
/// default; an operator terminating these headers at a proxy can turn the layer
/// off. Per-header policy is a single default set today; finer control is a
/// follow-up.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HelmetConfig {
    /// Whether the security-header layer is installed.
    #[toml_comment(inline)]
    pub mode: HttpFeatureMode,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::{feature_mode::HttpFeatureMode, helmet::HelmetConfig};

    #[test]
    fn helmet_enabled_by_default() {
        pretty_assertions::assert_eq!(HelmetConfig::default().mode, HttpFeatureMode::Enabled);
    }

    #[test]
    fn helmet_config_round_trips_through_toml() {
        let config = HelmetConfig {
            mode: HttpFeatureMode::Disabled,
        };
        let rendered = toml::to_string(&config).expect("helmet serialises");
        let parsed: HelmetConfig = toml::from_str(&rendered).expect("helmet parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
