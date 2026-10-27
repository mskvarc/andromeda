//! The security response fields the broker sets on its API.
//!
//! Each field is a setting of its own, so an operator can change or drop one
//! field while the rest keep their defaults. A field with a fixed set of values
//! is an enum whose `disabled` choice leaves the field out; the two fields with
//! free-form or compound values are tables with a `mode` of their own.

pub mod content_security_policy;
pub mod header_choices;
pub mod strict_transport_security;

use crate::schema::{
    feature_mode::FeatureMode,
    http::security_headers::{
        content_security_policy::ContentSecurityPolicyConfig,
        header_choices::{
            CrossOriginOpenerPolicySetting,
            CrossOriginResourcePolicySetting,
            OriginAgentClusterSetting,
            ReferrerPolicySetting,
            XContentTypeOptionsSetting,
            XDnsPrefetchControlSetting,
            XDownloadOptionsSetting,
            XFrameOptionsSetting,
            XPermittedCrossDomainPoliciesSetting,
            XXssProtectionSetting,
        },
        strict_transport_security::StrictTransportSecurityConfig,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The security response fields the broker sets on the API subtree.
///
/// `mode` switches the whole set; every other key sets one field. The defaults
/// are the `axum-helmet` defaults, except `Strict-Transport-Security`, which is
/// sent only when an operator turns it on.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "The security headers API responses carry, each set or turned off on its own.")]
pub struct SecurityHeadersConfig {
    /// Whether any security response field is set: `enabled` or `disabled`.
    #[schemars(
        description = "Whether API responses carry the security headers below. Turn it off when a proxy in front sets them all; to drop one header, set its own key to `\"disabled\"`."
    )]
    pub mode: FeatureMode,
    /// The `Content-Security-Policy` field.
    #[schemars(description = "The `Content-Security-Policy` header.")]
    pub content_security_policy: ContentSecurityPolicyConfig,
    /// The `Strict-Transport-Security` field, sent only over a TLS listener.
    #[schemars(description = "The `Strict-Transport-Security` header, sent only on a response to a request that arrived over the TLS listener.")]
    pub strict_transport_security: StrictTransportSecurityConfig,
    /// The `Cross-Origin-Opener-Policy` field.
    #[schemars(description = "The `Cross-Origin-Opener-Policy` header.")]
    pub cross_origin_opener_policy: CrossOriginOpenerPolicySetting,
    /// The `Cross-Origin-Resource-Policy` field.
    #[schemars(description = "The `Cross-Origin-Resource-Policy` header.")]
    pub cross_origin_resource_policy: CrossOriginResourcePolicySetting,
    /// The `Origin-Agent-Cluster` field.
    #[schemars(description = "The `Origin-Agent-Cluster` header.")]
    pub origin_agent_cluster: OriginAgentClusterSetting,
    /// The `Referrer-Policy` field.
    #[schemars(description = "The `Referrer-Policy` header.")]
    pub referrer_policy: ReferrerPolicySetting,
    /// The `X-Content-Type-Options` field.
    #[schemars(description = "The `X-Content-Type-Options` header.")]
    pub x_content_type_options: XContentTypeOptionsSetting,
    /// The `X-DNS-Prefetch-Control` field.
    #[schemars(description = "The `X-DNS-Prefetch-Control` header.")]
    pub x_dns_prefetch_control: XDnsPrefetchControlSetting,
    /// The `X-Download-Options` field.
    #[schemars(description = "The `X-Download-Options` header.")]
    pub x_download_options: XDownloadOptionsSetting,
    /// The `X-Frame-Options` field.
    #[schemars(description = "The `X-Frame-Options` header.")]
    pub x_frame_options: XFrameOptionsSetting,
    /// The `X-Permitted-Cross-Domain-Policies` field.
    #[schemars(description = "The `X-Permitted-Cross-Domain-Policies` header.")]
    pub x_permitted_cross_domain_policies: XPermittedCrossDomainPoliciesSetting,
    /// The `X-XSS-Protection` field.
    #[schemars(description = "The `X-XSS-Protection` header.")]
    pub x_xss_protection: XXssProtectionSetting,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        feature_mode::FeatureMode,
        http::security_headers::{
            SecurityHeadersConfig,
            header_choices::{ReferrerPolicySetting, XFrameOptionsSetting},
        },
    };

    #[test]
    fn security_headers_are_enabled_by_default() {
        pretty_assertions::assert_eq!(SecurityHeadersConfig::default().mode, FeatureMode::Enabled);
    }

    #[test]
    fn strict_transport_security_is_off_by_default() {
        pretty_assertions::assert_eq!(SecurityHeadersConfig::default().strict_transport_security.mode, FeatureMode::Disabled);
    }

    #[test]
    fn the_table_round_trips_through_toml() {
        let config = SecurityHeadersConfig {
            referrer_policy: ReferrerPolicySetting::StrictOriginWhenCrossOrigin,
            x_frame_options: XFrameOptionsSetting::Disabled,
            ..SecurityHeadersConfig::default()
        };
        let rendered = toml::to_string(&config).expect("the table serialises");
        let parsed: SecurityHeadersConfig = toml::from_str(&rendered).expect("the table parses");
        pretty_assertions::assert_eq!(parsed, config);
    }

    #[test]
    fn one_header_is_set_by_its_own_key() {
        let parsed: SecurityHeadersConfig = toml::from_str(
            &toml::to_string(&SecurityHeadersConfig::default())
                .expect("the defaults serialise")
                .replace("x_frame_options = \"same-origin\"", "x_frame_options = \"deny\""),
        )
        .expect("the table parses");
        pretty_assertions::assert_eq!(parsed.x_frame_options, XFrameOptionsSetting::Deny);
        pretty_assertions::assert_eq!(parsed.referrer_policy, ReferrerPolicySetting::NoReferrer);
    }
}
