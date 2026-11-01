//! The HTTP methods and header names a CORS policy names, read as the types
//! the CORS layer takes.
//!
//! The configuration holds each as the token the operator wrote; the method
//! and field-name grammars are the HTTP layer's, so they are applied here, and
//! the configuration loader asks for them so a policy naming a malformed token
//! is refused before the broker starts.

use andromeda_configuration::schema::http::cors::{CorsConfig, CorsHeaderName, CorsMethod, ExposeRule, HeaderRule, MethodRule};
use axum::http::{HeaderName, Method, header::InvalidHeaderName, method::InvalidMethod};
use thiserror::Error;

/// A CORS policy names a token the HTTP layer cannot read.
#[derive(Debug, Error)]
pub enum CorsTokenError {
    /// A listed method is not an HTTP method token.
    #[error("http.cors.allow_methods lists a value that is not an HTTP method: {0}")]
    Method(#[source] InvalidMethod),
    /// A listed header name is not an HTTP field name.
    #[error("http.cors lists a header that is not an HTTP field name: {0}")]
    HeaderName(#[source] InvalidHeaderName),
}

/// The configured method as an HTTP method.
///
/// # Errors
///
/// Returns [`CorsTokenError::Method`] when the token is not a method.
pub fn cors_method(method: &CorsMethod) -> Result<Method, CorsTokenError> {
    Method::from_bytes(method.as_str().as_bytes()).map_err(CorsTokenError::Method)
}

/// The configured header name as an HTTP field name.
///
/// # Errors
///
/// Returns [`CorsTokenError::HeaderName`] when the name is not a field name.
pub fn cors_header_name(name: &CorsHeaderName) -> Result<HeaderName, CorsTokenError> {
    HeaderName::from_bytes(name.as_str().as_bytes()).map_err(CorsTokenError::HeaderName)
}

/// Read every method and header name the policy lists.
///
/// # Errors
///
/// Returns the [`CorsTokenError`] of the first token that does not read.
pub fn check_cors_tokens(config: &CorsConfig) -> Result<(), CorsTokenError> {
    if let MethodRule::List(methods) = &config.allow_methods {
        methods.iter().try_for_each(|method| cors_method(method).map(drop))?;
    }
    let allowed = match &config.allow_headers {
        HeaderRule::List(names) => names.as_slice(),
        HeaderRule::Any | HeaderRule::Mirror => &[],
    };
    let exposed = match &config.expose_headers {
        ExposeRule::List(names) => names.as_slice(),
        ExposeRule::Any => &[],
    };
    allowed.iter().chain(exposed).try_for_each(|name| cors_header_name(name).map(drop))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::cors_tokens::{CorsTokenError, check_cors_tokens};
    use andromeda_configuration::schema::http::cors::CorsConfig;

    fn policy(body: &str) -> CorsConfig {
        toml::from_str(body).expect("a cors table")
    }

    #[test]
    fn listed_methods_and_header_names_read() {
        let config = policy(
            "mode = \"enabled\"\nallow_methods = [\"GET\", \"POST\"]\nallow_headers = [\"content-type\"]\nexpose_headers = [\"x-trace-id\"]\nallow_origins = \"any\"\ncredentials = \"refused\"\nprivate_network = \"refused\"\n",
        );
        assert!(check_cors_tokens(&config).is_ok());
    }

    #[test]
    fn a_method_with_a_space_is_refused() {
        let config = policy(
            "mode = \"enabled\"\nallow_methods = [\"G ET\"]\nallow_origins = \"any\"\nallow_headers = \"any\"\nexpose_headers = []\ncredentials = \"refused\"\nprivate_network = \"refused\"\n",
        );
        assert!(matches!(check_cors_tokens(&config), Err(CorsTokenError::Method(_))));
    }

    #[test]
    fn an_allowed_or_exposed_header_name_with_a_colon_is_refused() {
        for (allow, expose) in [("[\"bad:name\"]", "[]"), ("\"any\"", "[\"bad:name\"]")] {
            let config = policy(&format!(
                "mode = \"enabled\"\nallow_methods = \"any\"\nallow_origins = \"any\"\nallow_headers = {allow}\nexpose_headers = {expose}\ncredentials = \"refused\"\nprivate_network = \"refused\"\n"
            ));
            assert!(matches!(check_cors_tokens(&config), Err(CorsTokenError::HeaderName(_))), "{allow} {expose}");
        }
    }
}
