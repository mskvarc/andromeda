//! The `[http]` rules.

use crate::{
    schema::{
        feature_mode::FeatureMode,
        http::{
            cors::{CorsMode, CorsPermission},
            forwarded::ForwardedTrust,
        },
    },
    validate::{
        choice_value::ChoiceValue,
        compound_rule::{CompoundRule, Condition, Obligation},
        config_key::ConfigKey,
    },
};

/// The `[http]` rules that are no single comparison.
///
/// A decode threshold past the address width would never send a payload to
/// the blocking pool. The Fetch standard forbids credentials beside a CORS
/// wildcard, a combination the CORS layer cannot build; the profile modes
/// read neither, so only `configured` is held to it. A `proxies` trust with
/// no proxy would believe no one, and proxies beside any other trust would be
/// read by nothing, so either is an operator's mistake. Compression and
/// decompression are turned off by their `mode`, so an empty coding list
/// beside a mode that is on is a list that says nothing.
pub(crate) const COMPOUND_RULES: [CompoundRule; 6] = [
    CompoundRule {
        statement: "It must fit the platform's address width.",
        when: &[],
        then: &[Obligation::FitsAddressWidth(ConfigKey::HttpBodyDecodingBlockingThreshold)],
    },
    CompoundRule {
        statement: "With `http.cors.mode` at `\"configured\"`, `http.cors.credentials` at `\"allowed\"` cannot be combined with `\"any\"` in an allow or expose list.",
        when: &[
            Condition::Is {
                key: ConfigKey::HttpCorsMode,
                choices: &[ChoiceValue::Cors(CorsMode::Configured)],
            },
            Condition::Is {
                key: ConfigKey::HttpCorsCredentials,
                choices: &[ChoiceValue::CorsPermission(CorsPermission::Allowed)],
            },
        ],
        then: &[
            not_any(ConfigKey::HttpCorsAllowOrigins),
            not_any(ConfigKey::HttpCorsAllowMethods),
            not_any(ConfigKey::HttpCorsAllowHeaders),
            not_any(ConfigKey::HttpCorsExposeHeaders),
        ],
    },
    CompoundRule {
        statement: "`http.forwarded.proxies` must list at least one proxy while `http.forwarded.trust` is `\"proxies\"`.",
        when: &[Condition::Is {
            key: ConfigKey::HttpForwardedTrust,
            choices: &[ChoiceValue::ForwardedTrust(ForwardedTrust::Proxies)],
        }],
        then: &[Obligation::NonEmpty(ConfigKey::HttpForwardedProxies)],
    },
    CompoundRule {
        statement: "`http.forwarded.proxies` must be empty while `http.forwarded.trust` is not `\"proxies\"`.",
        when: &[Condition::IsNot {
            key: ConfigKey::HttpForwardedTrust,
            choice: ChoiceValue::ForwardedTrust(ForwardedTrust::Proxies),
        }],
        then: &[Obligation::Empty(ConfigKey::HttpForwardedProxies)],
    },
    CompoundRule {
        statement: "`http.compression.algorithms` must list at least one coding while `http.compression.mode` is `\"enabled\"`.",
        when: &[Condition::Is {
            key: ConfigKey::HttpCompressionMode,
            choices: &[ChoiceValue::Feature(FeatureMode::Enabled)],
        }],
        then: &[Obligation::NonEmpty(ConfigKey::HttpCompressionAlgorithms)],
    },
    CompoundRule {
        statement: "`http.decompression.algorithms` must list at least one coding while `http.decompression.mode` is `\"enabled\"`.",
        when: &[Condition::Is {
            key: ConfigKey::HttpDecompressionMode,
            choices: &[ChoiceValue::Feature(FeatureMode::Enabled)],
        }],
        then: &[Obligation::NonEmpty(ConfigKey::HttpDecompressionAlgorithms)],
    },
];

/// The CORS list `key` is not the wildcard `"any"`.
const fn not_any(key: ConfigKey) -> Obligation {
    Obligation::IsNot { key, choice: ChoiceValue::Any }
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        schema::{
            feature_mode::FeatureMode,
            http::{
                cors::{CorsMode, CorsPermission, ExposeRule, HeaderRule, MethodRule, OriginRule},
                forwarded::ForwardedTrust,
            },
        },
        validate::{
            address_width::AddressWidth,
            choice_value::ChoiceValue,
            config_key::ConfigKey,
            rule_tests::{assert_accepted, compound_refusals, refusals},
            violation::{ConfigurationViolation, HeldChoice, Requirement},
        },
    };
    use enumset::EnumSet;
    use pretty_assertions::assert_eq;
    use std::error::Error;
    use url_macro::url;

    /// A change setting one CORS list to `"any"`.
    type SetWildcard = fn(&mut BrokerConfig);

    /// The trust `trust`, as a refusal names it.
    fn held_trust(trust: ForwardedTrust) -> Requirement {
        Requirement::new([HeldChoice::new(ConfigKey::HttpForwardedTrust, trust)])
    }

    #[test]
    fn a_proxies_trust_without_a_proxy_is_refused_and_with_one_accepted() -> Result<(), Box<dyn Error>> {
        let mut config = BrokerConfig::local();
        config.http.forwarded.trust = ForwardedTrust::Proxies;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Unset {
                key: ConfigKey::HttpForwardedProxies,
                required_by: held_trust(ForwardedTrust::Proxies),
            }]
        );
        config.http.forwarded.proxies = vec!["10.0.0.0/8".parse()?];
        assert_accepted(&config);
        Ok(())
    }

    #[test]
    fn proxies_beside_a_trust_that_reads_no_list_are_refused() -> Result<(), Box<dyn Error>> {
        for trust in [ForwardedTrust::None, ForwardedTrust::Any] {
            let mut config = BrokerConfig::local();
            config.http.forwarded.trust = trust;
            config.http.forwarded.proxies = vec!["10.0.0.0/8".parse()?];
            assert_eq!(
                refusals(&config),
                [ConfigurationViolation::Unread {
                    key: ConfigKey::HttpForwardedProxies,
                    required_by: held_trust(trust),
                }]
            );
            config.http.forwarded.proxies.clear();
            assert_accepted(&config);
        }
        Ok(())
    }

    #[test]
    fn the_forwarded_refusals_name_both_keys() {
        assert_eq!(
            ConfigurationViolation::Unread {
                key: ConfigKey::HttpForwardedProxies,
                required_by: held_trust(ForwardedTrust::Any),
            }
            .to_string(),
            "http.forwarded.proxies must be left out while http.forwarded.trust is \"any\""
        );
        assert_eq!(
            ConfigurationViolation::Unset {
                key: ConfigKey::HttpForwardedProxies,
                required_by: held_trust(ForwardedTrust::Proxies),
            }
            .to_string(),
            "http.forwarded.proxies must be set while http.forwarded.trust is \"proxies\""
        );
    }

    /// A credentialed configured policy with every list at its default.
    fn credentialed_policy() -> BrokerConfig {
        let mut config = BrokerConfig::local();
        config.http.cors.mode = CorsMode::Configured;
        config.http.cors.credentials = CorsPermission::Allowed;
        config.http.cors.allow_origins = OriginRule::List(vec![url!("https://ui.example")]);
        config.http.cors.allow_methods = MethodRule::List(Vec::new());
        config.http.cors.allow_headers = HeaderRule::List(Vec::new());
        config.http.cors.expose_headers = ExposeRule::List(Vec::new());
        config
    }

    /// The refusal of the CORS list `key` at `"any"` beside credentials.
    fn wildcard_beside_credentials(key: ConfigKey) -> ConfigurationViolation {
        ConfigurationViolation::ChoiceRefused {
            key,
            choice: ChoiceValue::Any,
            required_by: Requirement::new([
                HeldChoice::new(ConfigKey::HttpCorsMode, CorsMode::Configured),
                HeldChoice::new(ConfigKey::HttpCorsCredentials, CorsPermission::Allowed),
            ]),
        }
    }

    #[test]
    fn each_cors_list_at_any_beside_credentials_is_refused_and_listed_accepted() {
        assert_accepted(&credentialed_policy());
        let wildcards: [(SetWildcard, ConfigKey); 4] = [
            (|config| config.http.cors.allow_origins = OriginRule::Any, ConfigKey::HttpCorsAllowOrigins),
            (|config| config.http.cors.allow_methods = MethodRule::Any, ConfigKey::HttpCorsAllowMethods),
            (|config| config.http.cors.allow_headers = HeaderRule::Any, ConfigKey::HttpCorsAllowHeaders),
            (|config| config.http.cors.expose_headers = ExposeRule::Any, ConfigKey::HttpCorsExposeHeaders),
        ];
        for (set_any, key) in wildcards {
            let mut config = credentialed_policy();
            set_any(&mut config);
            assert_eq!(refusals(&config), [wildcard_beside_credentials(key)], "{key}");
        }
    }

    /// A mirrored method or header list names a concrete value per request,
    /// which credentials allow.
    #[test]
    fn a_mirrored_cors_list_beside_credentials_is_accepted() {
        let mut config = credentialed_policy();
        config.http.cors.allow_methods = MethodRule::Mirror;
        config.http.cors.allow_headers = HeaderRule::Mirror;
        assert_accepted(&config);
    }

    /// The profile modes read neither the credentials nor the lists. Every
    /// list but the exposed headers is `"any"` by default.
    #[test]
    fn wildcards_beside_credentials_are_accepted_outside_the_configured_mode() {
        for mode in [CorsMode::Permissive, CorsMode::VeryPermissive, CorsMode::Disabled] {
            let mut config = BrokerConfig::local();
            config.http.cors.mode = mode;
            config.http.cors.credentials = CorsPermission::Allowed;
            assert_accepted(&config);
        }
        let mut config = BrokerConfig::local();
        config.http.cors.mode = CorsMode::Configured;
        config.http.cors.credentials = CorsPermission::Allowed;
        assert_eq!(
            refusals(&config),
            [
                wildcard_beside_credentials(ConfigKey::HttpCorsAllowOrigins),
                wildcard_beside_credentials(ConfigKey::HttpCorsAllowMethods),
                wildcard_beside_credentials(ConfigKey::HttpCorsAllowHeaders),
            ]
        );
    }

    #[test]
    fn a_decode_threshold_past_a_32_bit_address_width_is_refused_and_one_within_accepted() {
        let mut config = BrokerConfig::local();
        config.http.body_decoding.blocking_threshold = HumanByteSize::gib(5);
        assert_eq!(
            compound_refusals(&config, AddressWidth::new(32)),
            [ConfigurationViolation::ExceedsAddressWidth {
                key: ConfigKey::HttpBodyDecodingBlockingThreshold,
                bytes: 5 << 30,
            }]
        );
        assert_eq!(compound_refusals(&config, AddressWidth::new(64)), []);
        config.http.body_decoding.blocking_threshold = HumanByteSize::mib(64);
        assert_eq!(compound_refusals(&config, AddressWidth::new(32)), []);
    }

    #[test]
    fn an_empty_coding_list_is_refused_while_its_table_is_on_and_accepted_while_off() {
        let mut config = BrokerConfig::local();
        config.http.compression.algorithms = EnumSet::empty();
        config.http.decompression.algorithms = EnumSet::empty();
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Unset {
                    key: ConfigKey::HttpCompressionAlgorithms,
                    required_by: Requirement::new([HeldChoice::new(ConfigKey::HttpCompressionMode, FeatureMode::Enabled)]),
                },
                ConfigurationViolation::Unset {
                    key: ConfigKey::HttpDecompressionAlgorithms,
                    required_by: Requirement::new([HeldChoice::new(ConfigKey::HttpDecompressionMode, FeatureMode::Enabled)]),
                },
            ]
        );
        config.http.compression.mode = FeatureMode::Disabled;
        config.http.decompression.mode = FeatureMode::Disabled;
        assert_accepted(&config);
    }
}
