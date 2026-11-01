//! The `Content-Security-Policy` response field.

use crate::schema::feature_mode::FeatureMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The `axum-helmet` default policy, as the field carries it.
const DEFAULT_POLICY: &str = "default-src 'self'; base-uri 'self'; font-src 'self' https: data:; form-action 'self'; frame-ancestors 'self'; img-src 'self' data:; object-src 'none'; script-src 'self'; script-src-attr 'none'; style-src 'self' https: 'unsafe-inline'; upgrade-insecure-requests";

/// A policy as the operator wrote it.
///
/// The field-value grammar belongs to the HTTP layer, which converts the
/// policy into its header-value type while the configuration loads, so a
/// policy no field can carry is refused there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(description = "A `Content-Security-Policy` value: directives separated by `;`.")]
pub struct ContentSecurityPolicyText(String);

impl ContentSecurityPolicyText {
    /// The policy as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether the `Content-Security-Policy` field is sent, and what it says.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The `Content-Security-Policy` header.")]
pub struct ContentSecurityPolicyConfig {
    /// Whether the field is sent: `enabled` or `disabled`.
    #[schemars(description = "Whether API responses carry a `Content-Security-Policy` header.")]
    pub mode: FeatureMode,
    /// The field value.
    #[schemars(description = "The policy the header carries.")]
    pub policy: ContentSecurityPolicyText,
}

impl Default for ContentSecurityPolicyConfig {
    fn default() -> ContentSecurityPolicyConfig {
        ContentSecurityPolicyConfig {
            mode: FeatureMode::Enabled,
            policy: ContentSecurityPolicyText(DEFAULT_POLICY.to_owned()),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{feature_mode::FeatureMode, http::security_headers::content_security_policy::ContentSecurityPolicyConfig};

    #[test]
    fn the_policy_is_sent_by_default() {
        pretty_assertions::assert_eq!(ContentSecurityPolicyConfig::default().mode, FeatureMode::Enabled);
    }

    #[test]
    fn a_policy_is_read_as_written() {
        let parsed: ContentSecurityPolicyConfig = toml::from_str("mode = \"enabled\"\npolicy = \"default-src 'none'\"").expect("the table parses");
        pretty_assertions::assert_eq!(parsed.policy.as_str(), "default-src 'none'");
    }
}
