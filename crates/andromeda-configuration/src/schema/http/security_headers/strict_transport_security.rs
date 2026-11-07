//! The `Strict-Transport-Security` response field.

use crate::{human_duration::HumanDuration, schema::feature_mode::FeatureMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 180 days, the `axum-helmet` default `max-age`.
const DEFAULT_MAX_AGE: HumanDuration = HumanDuration::from_days(180);

/// Whether the `Strict-Transport-Security` field is sent, and what it says.
///
/// Off by default: the field pins every later visit of a browser to HTTPS for
/// `max_age`, and with `includeSubDomains` every subdomain of the broker's host
/// too, so only an operator who knows the host is served over TLS for that long
/// turns it on.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The `Strict-Transport-Security` header.")]
pub struct StrictTransportSecurityConfig {
    /// Whether the field is sent over the TLS listener: `enabled` or `disabled`.
    #[schemars(
        description = "Whether a response to a request over the TLS listener carries `Strict-Transport-Security`. A browser that sees it uses HTTPS for the host until `max_age` passes. Off by default, since a browser that has seen it cannot reach the host over plain HTTP again until then; turn it on once HTTPS is the only way the host is reached."
    )]
    pub mode: FeatureMode,
    /// How long a browser remembers to use HTTPS for the host.
    #[schemars(description = "How long a browser keeps using HTTPS for the host, the header's `max-age`. Zero tells a browser to forget the host.")]
    pub max_age: HumanDuration,
    /// Whether the policy covers every subdomain of the host.
    #[schemars(description = "Whether the policy also covers every subdomain of the host, the header's `includeSubDomains`.")]
    pub include_subdomains: FeatureMode,
}

impl Default for StrictTransportSecurityConfig {
    fn default() -> StrictTransportSecurityConfig {
        StrictTransportSecurityConfig {
            mode: FeatureMode::Disabled,
            max_age: DEFAULT_MAX_AGE,
            include_subdomains: FeatureMode::Disabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::{feature_mode::FeatureMode, http::security_headers::strict_transport_security::StrictTransportSecurityConfig},
    };

    #[test]
    fn the_field_and_its_subdomains_are_off_by_default() {
        let config = StrictTransportSecurityConfig::default();
        pretty_assertions::assert_eq!(config.mode, FeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.include_subdomains, FeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.max_age, HumanDuration::from_days(180));
    }

    #[test]
    fn the_table_round_trips_through_toml() {
        let config = StrictTransportSecurityConfig {
            mode: FeatureMode::Enabled,
            max_age: HumanDuration::from_days(365),
            include_subdomains: FeatureMode::Enabled,
        };
        let rendered = toml::to_string(&config).expect("the table serialises");
        let parsed: StrictTransportSecurityConfig = toml::from_str(&rendered).expect("the table parses");
        pretty_assertions::assert_eq!(parsed, config);
    }
}
