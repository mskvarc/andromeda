//! The `[http]` rules.

use crate::{
    schema::http::{HttpStackConfig, forwarded::ForwardedTrust},
    validate::{
        config_key::ConfigKey,
        rule::CompoundRule,
        violation::{ConfigurationViolation, Requirement},
        violation_log::ViolationLog,
    },
};

/// What [`check_http`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 3] = [
    CompoundRule {
        keys: &[ConfigKey::HttpBodyDecodingBlockingThreshold],
        statement: "It must fit the platform's address width.",
    },
    CompoundRule {
        keys: &[
            ConfigKey::HttpCorsCredentials,
            ConfigKey::HttpCorsAllowOrigins,
            ConfigKey::HttpCorsAllowMethods,
            ConfigKey::HttpCorsAllowHeaders,
            ConfigKey::HttpCorsExposeHeaders,
        ],
        statement: "With `http.cors.mode` at `\"configured\"`, `http.cors.credentials` at `\"allowed\"` cannot be combined with `\"any\"` in an allow or expose list.",
    },
    CompoundRule {
        keys: &[ConfigKey::HttpForwardedTrust, ConfigKey::HttpForwardedProxies],
        statement: "`http.forwarded.proxies` must list at least one proxy with `http.forwarded.trust` at `\"proxies\"`, and must be empty with any other trust.",
    },
];

/// A decode threshold past the address width would never send a payload to
/// the blocking pool, and the Fetch standard forbids credentials beside a
/// CORS wildcard, a combination the CORS layer cannot build. A `proxies` trust
/// with no proxy would believe no one, and proxies beside any other trust
/// would be read by nothing, so either is an operator's mistake.
pub(crate) fn check_http(http: &HttpStackConfig, log: &mut ViolationLog) {
    if http.body_decoding.blocking_threshold_bytes().is_err() {
        log.record(ConfigurationViolation::ExceedsAddressWidth {
            key: ConfigKey::HttpBodyDecodingBlockingThreshold,
            bytes: http.body_decoding.blocking_threshold.as_u64(),
        });
    }
    if http.cors.has_credentialed_wildcard() {
        log.record(ConfigurationViolation::CorsCredentialsWithWildcard);
    }
    match (http.forwarded.trust, http.forwarded.proxies.is_empty()) {
        (ForwardedTrust::Proxies, true) => log.record(ConfigurationViolation::Unset {
            key: ConfigKey::HttpForwardedProxies,
            required_by: Requirement::ProxyTrust,
        }),
        (trust @ (ForwardedTrust::None | ForwardedTrust::Any), false) => log.record(ConfigurationViolation::ProxiesWithoutProxyTrust { trust }),
        (ForwardedTrust::Proxies, false) | (ForwardedTrust::None | ForwardedTrust::Any, true) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        schema::http::{
            cors::{CorsMode, CorsPermission, ExposeRule, HeaderRule, MethodRule, OriginRule},
            forwarded::ForwardedTrust,
        },
        validate::{
            config_key::ConfigKey,
            rule_tests::{assert_accepted, refusals},
            violation::{ConfigurationViolation, Requirement},
        },
    };
    use pretty_assertions::assert_eq;
    use std::error::Error;
    use url_macro::url;

    #[test]
    fn a_proxies_trust_without_a_proxy_is_refused_and_with_one_accepted() -> Result<(), Box<dyn Error>> {
        let mut config = BrokerConfig::default();
        config.http.forwarded.trust = ForwardedTrust::Proxies;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Unset {
                key: ConfigKey::HttpForwardedProxies,
                required_by: Requirement::ProxyTrust,
            }]
        );
        config.http.forwarded.proxies = vec!["10.0.0.0/8".parse()?];
        assert_accepted(&config);
        Ok(())
    }

    #[test]
    fn proxies_beside_a_trust_that_reads_no_list_are_refused() -> Result<(), Box<dyn Error>> {
        for trust in [ForwardedTrust::None, ForwardedTrust::Any] {
            let mut config = BrokerConfig::default();
            config.http.forwarded.trust = trust;
            config.http.forwarded.proxies = vec!["10.0.0.0/8".parse()?];
            assert_eq!(refusals(&config), [ConfigurationViolation::ProxiesWithoutProxyTrust { trust }]);
            config.http.forwarded.proxies.clear();
            assert_accepted(&config);
        }
        Ok(())
    }

    #[test]
    fn the_forwarded_refusals_name_both_keys() {
        assert_eq!(
            ConfigurationViolation::ProxiesWithoutProxyTrust { trust: ForwardedTrust::Any }.to_string(),
            "http.forwarded.proxies is set but http.forwarded.trust is \"any\"; set the trust to \"proxies\" or remove the list"
        );
        assert_eq!(
            ConfigurationViolation::Unset {
                key: ConfigKey::HttpForwardedProxies,
                required_by: Requirement::ProxyTrust,
            }
            .to_string(),
            "http.forwarded.proxies must be set while http.forwarded.trust is \"proxies\""
        );
    }

    #[test]
    fn credentials_beside_a_wildcard_are_refused_and_beside_listed_rules_accepted() {
        let mut config = BrokerConfig::default();
        config.http.cors.mode = CorsMode::Configured;
        config.http.cors.credentials = CorsPermission::Allowed;
        config.http.cors.allow_origins = OriginRule::Any;
        assert_eq!(refusals(&config), [ConfigurationViolation::CorsCredentialsWithWildcard]);
        config.http.cors.allow_origins = OriginRule::List(vec![url!("https://ui.example")]);
        config.http.cors.allow_methods = MethodRule::List(Vec::new());
        config.http.cors.allow_headers = HeaderRule::List(Vec::new());
        config.http.cors.expose_headers = ExposeRule::List(Vec::new());
        assert_accepted(&config);
    }

    /// The address width is checked only where `usize` is narrower than
    /// `u64`, the only platforms a byte size can pass it on.
    #[test]
    fn a_decode_threshold_past_the_address_width_is_refused() {
        let mut config = BrokerConfig::default();
        config.http.body_decoding.blocking_threshold = HumanByteSize::b(u64::MAX);
        if usize::try_from(u64::MAX).is_ok() {
            assert_accepted(&config);
        } else {
            assert_eq!(
                refusals(&config),
                [ConfigurationViolation::ExceedsAddressWidth {
                    key: ConfigKey::HttpBodyDecodingBlockingThreshold,
                    bytes: u64::MAX,
                }]
            );
        }
    }
}
