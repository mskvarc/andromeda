//! Mapping of the CORS configuration onto a `tower-http` CORS layer.

use crate::cors_tokens::{CorsTokenError, cors_header_name, cors_method};
use andromeda_configuration::schema::http::cors::{CorsConfig, CorsMode, CorsPermission, ExposeRule, HeaderRule, MethodRule, OriginRule};
use axum::http::HeaderValue;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer, ExposeHeaders};
use url::Url;

/// Build the CORS layer the configuration calls for, or `None` when CORS is off.
///
/// `permissive` and `very-permissive` return the corresponding complete profiles
/// and ignore every custom field; `configured` builds the layer from the custom
/// fields; `disabled` installs nothing.
///
/// # Errors
///
/// Returns the [`CorsTokenError`] of a listed method or header name the HTTP
/// layer cannot read.
pub(crate) fn build_cors_layer(config: &CorsConfig) -> Result<Option<CorsLayer>, CorsTokenError> {
    Ok(match config.mode {
        CorsMode::Disabled => None,
        CorsMode::Permissive => Some(CorsLayer::permissive()),
        CorsMode::VeryPermissive => Some(CorsLayer::very_permissive()),
        CorsMode::Configured => Some(build_configured_cors_layer(config)?),
    })
}

/// Assemble the custom CORS layer from the `configured`-mode fields.
fn build_configured_cors_layer(config: &CorsConfig) -> Result<CorsLayer, CorsTokenError> {
    let mut layer = CorsLayer::new()
        .allow_origin(origin_rule(&config.allow_origins))
        .allow_methods(method_rule(&config.allow_methods)?)
        .allow_headers(header_rule(&config.allow_headers)?)
        .expose_headers(expose_rule(&config.expose_headers)?)
        .allow_credentials(matches!(config.credentials, CorsPermission::Allowed))
        .allow_private_network(matches!(config.private_network, CorsPermission::Allowed));
    if let Some(lifetime) = config.preflight_lifetime {
        layer = layer.max_age(lifetime.duration());
    }
    Ok(layer)
}

/// Translate an origin rule into the layer's origin policy.
fn origin_rule(rule: &OriginRule) -> AllowOrigin {
    match rule {
        OriginRule::Any => AllowOrigin::any(),
        OriginRule::List(origins) => AllowOrigin::list(origins.iter().filter_map(origin_header_value)),
    }
}

/// Translate a method rule into the layer's method policy.
fn method_rule(rule: &MethodRule) -> Result<AllowMethods, CorsTokenError> {
    Ok(match rule {
        MethodRule::Any => AllowMethods::any(),
        MethodRule::Mirror => AllowMethods::mirror_request(),
        MethodRule::List(methods) => AllowMethods::list(methods.iter().map(cors_method).collect::<Result<Vec<_>, _>>()?),
    })
}

/// Translate a request-header rule into the layer's allowed-header policy.
fn header_rule(rule: &HeaderRule) -> Result<AllowHeaders, CorsTokenError> {
    Ok(match rule {
        HeaderRule::Any => AllowHeaders::any(),
        HeaderRule::Mirror => AllowHeaders::mirror_request(),
        HeaderRule::List(headers) => AllowHeaders::list(headers.iter().map(cors_header_name).collect::<Result<Vec<_>, _>>()?),
    })
}

/// Translate an expose rule into the layer's exposed-header policy.
fn expose_rule(rule: &ExposeRule) -> Result<ExposeHeaders, CorsTokenError> {
    Ok(match rule {
        ExposeRule::Any => ExposeHeaders::any(),
        ExposeRule::List(headers) => ExposeHeaders::list(headers.iter().map(cors_header_name).collect::<Result<Vec<_>, _>>()?),
    })
}

/// The ASCII serialisation of a URL's origin as a header value.
///
/// A validated [`Url`] always has an ASCII-serialisable origin that is a valid
/// header value, so a rejected value is dropped rather than aborting the build.
fn origin_header_value(url: &Url) -> Option<HeaderValue> {
    HeaderValue::from_str(&url.origin().ascii_serialization()).ok()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stack::cors_layer::build_cors_layer;
    use andromeda_configuration::schema::http::cors::{CorsConfig, CorsMode, CorsPermission, HeaderRule, MethodRule, OriginRule};

    #[test]
    fn cors_disabled_builds_no_layer() {
        let config = CorsConfig {
            mode: CorsMode::Disabled,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).expect("a readable policy").is_none());
    }

    #[test]
    fn cors_permissive_ignores_custom_settings() {
        // A credentialed wildcard would panic under `configured`; the profile ignores
        // it, so building the permissive layer must succeed regardless.
        let config = CorsConfig {
            mode: CorsMode::Permissive,
            credentials: CorsPermission::Allowed,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).expect("a readable policy").is_some());
    }

    #[test]
    fn cors_very_permissive_ignores_custom_settings() {
        let config = CorsConfig {
            mode: CorsMode::VeryPermissive,
            credentials: CorsPermission::Allowed,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(build_cors_layer(&config).expect("a readable policy").is_some());
    }

    #[test]
    fn cors_configured_builds_from_custom_origins_methods_headers() {
        let config: CorsConfig = toml::from_str(
            r#"
mode = "configured"
allow_origins = ["https://ui.example"]
allow_methods = ["GET", "POST"]
allow_headers = ["content-type"]
expose_headers = ["x-trace-id"]
credentials = "allowed"
private_network = "refused"
"#,
        )
        .expect("cors config parses");
        assert!(matches!(config.allow_origins, OriginRule::List(_)));
        assert!(matches!(config.allow_methods, MethodRule::List(_)));
        assert!(matches!(config.allow_headers, HeaderRule::List(_)));
        assert!(build_cors_layer(&config).expect("a readable policy").is_some());
    }
}
