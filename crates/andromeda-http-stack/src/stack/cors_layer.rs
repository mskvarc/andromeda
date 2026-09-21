//! Mapping of the CORS configuration onto a `tower-http` CORS layer.

use crate::config::cors::{CorsConfig, CorsHeaderName, CorsMethod, CorsMode, ExposeRule, HeaderRule, MethodRule, OriginRule};
use axum::http::HeaderValue;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer, ExposeHeaders};
use url::Url;

/// Build the CORS layer the configuration calls for, or `None` when CORS is off.
///
/// `permissive` and `very-permissive` return the corresponding complete profiles
/// and ignore every custom field; `enabled` builds the layer from the custom
/// fields; `disabled` installs nothing.
pub(crate) fn build_cors_layer(config: &CorsConfig) -> Option<CorsLayer> {
    match config.mode {
        CorsMode::Disabled => None,
        CorsMode::Permissive => Some(CorsLayer::permissive()),
        CorsMode::VeryPermissive => Some(CorsLayer::very_permissive()),
        CorsMode::Enabled => Some(build_enabled_cors_layer(config)),
    }
}

/// Assemble the custom CORS layer from the `enabled`-mode fields.
fn build_enabled_cors_layer(config: &CorsConfig) -> CorsLayer {
    let mut layer = CorsLayer::new()
        .allow_origin(origin_rule(&config.allow_origins))
        .allow_methods(method_rule(&config.allow_methods))
        .allow_headers(header_rule(&config.allow_headers))
        .expose_headers(expose_rule(&config.expose_headers))
        .allow_credentials(config.allow_credentials)
        .allow_private_network(config.allow_private_network);
    if let Some(max_age) = config.max_age {
        layer = layer.max_age(max_age.duration());
    }
    layer
}

/// Translate an origin rule into the layer's origin policy.
fn origin_rule(rule: &OriginRule) -> AllowOrigin {
    match rule {
        OriginRule::Any => AllowOrigin::any(),
        OriginRule::List(origins) => AllowOrigin::list(origins.iter().filter_map(origin_header_value)),
    }
}

/// Translate a method rule into the layer's method policy.
fn method_rule(rule: &MethodRule) -> AllowMethods {
    match rule {
        MethodRule::Any => AllowMethods::any(),
        MethodRule::Mirror => AllowMethods::mirror_request(),
        MethodRule::List(methods) => AllowMethods::list(methods.iter().map(CorsMethod::method)),
    }
}

/// Translate a request-header rule into the layer's allowed-header policy.
fn header_rule(rule: &HeaderRule) -> AllowHeaders {
    match rule {
        HeaderRule::Any => AllowHeaders::any(),
        HeaderRule::Mirror => AllowHeaders::mirror_request(),
        HeaderRule::List(headers) => AllowHeaders::list(headers.iter().map(CorsHeaderName::header_name)),
    }
}

/// Translate an expose rule into the layer's exposed-header policy.
fn expose_rule(rule: &ExposeRule) -> ExposeHeaders {
    match rule {
        ExposeRule::Any => ExposeHeaders::any(),
        ExposeRule::List(headers) => ExposeHeaders::list(headers.iter().map(CorsHeaderName::header_name)),
    }
}

/// The ASCII serialisation of a URL's origin as a header value.
///
/// A validated [`Url`] always has an ASCII-serialisable origin that is a valid
/// header value, so a rejected value is dropped rather than aborting the build.
fn origin_header_value(url: &Url) -> Option<HeaderValue> {
    HeaderValue::from_str(&url.origin().ascii_serialization()).ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        config::cors::{CorsConfig, CorsMode, HeaderRule, MethodRule, OriginRule},
        stack::cors_layer::build_cors_layer,
    };

    #[test]
    fn cors_disabled_builds_no_layer() {
        let config = CorsConfig {
            mode: CorsMode::Disabled,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).is_none());
    }

    #[test]
    fn cors_permissive_ignores_custom_settings() {
        // A credentialed wildcard would panic under `enabled`; the profile ignores
        // it, so building the permissive layer must succeed regardless.
        let config = CorsConfig {
            mode: CorsMode::Permissive,
            allow_credentials: true,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).is_some());
    }

    #[test]
    fn cors_very_permissive_ignores_custom_settings() {
        let config = CorsConfig {
            mode: CorsMode::VeryPermissive,
            allow_credentials: true,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).is_some());
    }

    #[test]
    fn cors_enabled_builds_from_custom_origins_methods_headers() {
        let config: CorsConfig = toml::from_str(
            r#"
mode = "enabled"
allow_origins = ["https://ui.example"]
allow_methods = ["GET", "POST"]
allow_headers = ["content-type"]
expose_headers = ["x-trace-id"]
allow_credentials = true
allow_private_network = false
"#,
        )
        .expect("cors config parses");
        assert!(matches!(config.allow_origins, OriginRule::List(_)));
        assert!(matches!(config.allow_methods, MethodRule::List(_)));
        assert!(matches!(config.allow_headers, HeaderRule::List(_)));
        assert!(build_cors_layer(&config).is_some());
    }
}
