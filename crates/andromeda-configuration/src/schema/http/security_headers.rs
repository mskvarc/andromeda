//! The security response fields the broker sets on its API.

use crate::schema::feature_mode::FeatureMode;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// Whether the broker sets its security response fields.
///
/// The broker sets a default set of security response fields
/// (`Content-Security-Policy`, `X-Content-Type-Options`, the cross-origin
/// isolation trio, and the rest of the `axum-helmet` defaults) on the API
/// subtree. `Strict-Transport-Security` belongs to the set too, but a response
/// carries it only when the request arrived over a TLS listener. Enabled by
/// default; an operator setting these fields at a proxy can turn them off.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecurityHeadersConfig {
    /// Whether the security response fields are set: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{feature_mode::FeatureMode, http::security_headers::SecurityHeadersConfig};

    #[test]
    fn security_headers_are_enabled_by_default() {
        pretty_assertions::assert_eq!(SecurityHeadersConfig::default().mode, FeatureMode::Enabled);
    }

    #[test]
    fn the_table_round_trips_through_toml() {
        let config = SecurityHeadersConfig { mode: FeatureMode::Disabled };
        let rendered = toml::to_string(&config).expect("the table serialises");
        let parsed: SecurityHeadersConfig = toml::from_str(&rendered).expect("the table parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
