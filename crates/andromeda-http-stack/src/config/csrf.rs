//! Cross-Site Request Forgery protection policy.

use crate::config::feature_mode::HttpFeatureMode;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;
use url::Url;

/// CSRF protection configuration.
///
/// Off by default: the broker's primary clients are machine-to-machine, which
/// send no `Origin` and are unaffected, while a browser-driven UI in front of the
/// broker enables it and declares the origins it serves from.
#[derive(Debug, Clone, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
pub struct CsrfConfig {
    /// Whether the CSRF layer is installed.
    #[toml_comment(inline)]
    pub mode: HttpFeatureMode,
    /// Browser origins whose cross-site requests are trusted, in addition to
    /// same-origin requests which are always allowed.
    pub trusted_origins: Vec<Url>,
}

impl Default for CsrfConfig {
    fn default() -> CsrfConfig {
        CsrfConfig {
            mode: HttpFeatureMode::Disabled,
            trusted_origins: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{csrf::CsrfConfig, feature_mode::HttpFeatureMode};

    #[test]
    fn csrf_disabled_by_default() {
        pretty_assertions::assert_eq!(CsrfConfig::default().mode, HttpFeatureMode::Disabled);
        pretty_assertions::assert_eq!(CsrfConfig::default().trusted_origins, Vec::<url::Url>::new());
    }

    #[test]
    fn csrf_config_round_trips_through_toml() {
        let config = CsrfConfig {
            mode: HttpFeatureMode::Enabled,
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
