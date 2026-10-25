//! Construction of the CSRF middleware layer.

use crate::config::{csrf::CsrfConfig, feature_mode::HttpFeatureMode};
use thiserror::Error;
use tower_http::csrf::{ConfigError, CsrfLayer};

/// A CSRF trusted origin could not be admitted by the CSRF layer.
#[derive(Debug, Error)]
pub enum CsrfLayerError {
    /// A configured trusted origin was not a valid origin the layer accepts.
    #[error("invalid CSRF trusted origin: {0}")]
    TrustedOrigin(#[from] ConfigError),
}

/// Build the CSRF layer, or `None` when the policy disables it.
///
/// # Errors
///
/// Returns [`CsrfLayerError::TrustedOrigin`] when a configured trusted origin is
/// not one the CSRF layer accepts.
pub(crate) fn build_csrf_layer(config: &CsrfConfig) -> Result<Option<CsrfLayer>, CsrfLayerError> {
    match config.mode {
        HttpFeatureMode::Disabled => Ok(None),
        HttpFeatureMode::Enabled => {
            let mut layer = CsrfLayer::new();
            for origin in &config.trusted_origins {
                // The CSRF layer matches the request `Origin` header, which is a
                // bare origin (`scheme://host[:port]`), so the URL's origin
                // serialisation is passed rather than its full string form (which
                // carries a path, if only a trailing `/`).
                layer = layer.add_trusted_origin(origin.origin().ascii_serialization())?;
            }
            Ok(Some(layer))
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        config::{csrf::CsrfConfig, feature_mode::HttpFeatureMode},
        stack::csrf_layer::build_csrf_layer,
    };

    #[test]
    fn csrf_disabled_builds_no_layer() {
        let config = CsrfConfig {
            mode: HttpFeatureMode::Disabled,
            trusted_origins: Vec::new(),
        };
        assert!(build_csrf_layer(&config).expect("disabled csrf builds").is_none());
    }

    #[test]
    fn csrf_enabled_with_trusted_origins_builds_layer() {
        let config = CsrfConfig {
            mode: HttpFeatureMode::Enabled,
            trusted_origins: vec!["https://ui.example".parse().expect("origin URL")],
        };
        assert!(build_csrf_layer(&config).expect("enabled csrf builds").is_some());
    }
}
