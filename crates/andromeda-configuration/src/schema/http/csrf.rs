//! Cross-Site Request Forgery protection policy.

use crate::{comma_list::array_or_comma_list, schema::feature_mode::FeatureMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

/// CSRF protection configuration.
///
/// Off by default: the broker's primary clients are machine-to-machine, which
/// send no `Origin` and are unaffected, while a browser-driven UI in front of the
/// broker enables it and declares the origins it serves from.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(
    description = "Cross-Site Request Forgery protection, for a browser-driven interface in front of the broker. Machine clients send no `Origin` and are unaffected."
)]
pub struct CsrfConfig {
    /// Whether the CSRF layer is installed.
    #[schemars(
        description = "Whether a cross-site request from an origin outside `http.csrf.trusted_origins` is answered `403`. Same-origin requests, requests with neither `Origin` nor `Sec-Fetch-Site`, and `GET`, `HEAD` and `OPTIONS` always go through."
    )]
    pub mode: FeatureMode,
    /// Browser origins whose cross-site requests are trusted, in addition to
    /// same-origin requests which are always allowed.
    #[serde(deserialize_with = "array_or_comma_list")]
    #[schemars(description = "The browser origins whose cross-site requests are trusted, in addition to same-origin requests.")]
    pub trusted_origins: Vec<Url>,
}

impl Default for CsrfConfig {
    fn default() -> CsrfConfig {
        CsrfConfig {
            mode: FeatureMode::Disabled,
            trusted_origins: Vec::new(),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{feature_mode::FeatureMode, http::csrf::CsrfConfig};

    #[test]
    fn csrf_disabled_by_default() {
        pretty_assertions::assert_eq!(CsrfConfig::default().mode, FeatureMode::Disabled);
        pretty_assertions::assert_eq!(CsrfConfig::default().trusted_origins, Vec::<url::Url>::new());
    }

    #[test]
    fn csrf_config_round_trips_through_toml() {
        let config = CsrfConfig {
            mode: FeatureMode::Enabled,
            trusted_origins: vec!["https://ui.example".parse().expect("origin URL")],
        };
        let rendered = toml::to_string(&config).expect("csrf serialises");
        let parsed: CsrfConfig = toml::from_str(&rendered).expect("csrf parses");
        pretty_assertions::assert_eq!(parsed, config);
    }

    #[test]
    fn a_non_url_trusted_origin_is_rejected() {
        let result: Result<CsrfConfig, _> = toml::from_str("mode = \"enabled\"\ntrusted_origins = [\"not a url\"]");
        assert!(result.is_err());
    }
}
