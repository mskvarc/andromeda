//! The `[http]` rules.

use crate::{
    schema::http::HttpStackConfig,
    validate::{config_key::ConfigKey, violation::ConfigurationViolation, violation_log::ViolationLog},
};

/// A decode threshold past the address width would never send a payload to
/// the blocking pool, and the Fetch standard forbids credentials beside a
/// CORS wildcard, a combination the CORS layer cannot build.
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
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        schema::http::cors::{CorsMode, CorsPermission, ExposeRule, HeaderRule, MethodRule, OriginRule},
        validate::{
            config_key::ConfigKey,
            rule_tests::{assert_accepted, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use url_macro::url;

    #[test]
    fn credentials_beside_a_wildcard_are_refused_and_beside_listed_rules_accepted() {
        let mut config = BrokerConfig::default();
        config.http.cors.mode = CorsMode::Enabled;
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
