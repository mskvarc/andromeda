//! The `Strict-Transport-Security` response header, sent only over TLS.

use andromeda_configuration::schema::{feature_mode::FeatureMode, http::security_headers::SecurityHeadersConfig};
use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use headers::{HeaderMapExt, StrictTransportSecurity};

/// The policy `config` sends, or `None` when the security fields or this one
/// are turned off.
pub(crate) fn strict_transport_security(config: &SecurityHeadersConfig) -> Option<StrictTransportSecurity> {
    let policy = config.strict_transport_security;
    match (config.mode, policy.mode) {
        (FeatureMode::Enabled, FeatureMode::Enabled) => Some(match policy.include_subdomains {
            FeatureMode::Enabled => StrictTransportSecurity::including_subdomains(policy.max_age.duration()),
            FeatureMode::Disabled => StrictTransportSecurity::excluding_subdomains(policy.max_age.duration()),
        }),
        (FeatureMode::Disabled, FeatureMode::Enabled | FeatureMode::Disabled) | (FeatureMode::Enabled, FeatureMode::Disabled) => None,
    }
}

/// Sets `policy` on a response to a request that arrived over an `https`
/// listener, on every such response including errors.
///
/// RFC 6797 section 7.2 has a user agent ignore the field on a plaintext
/// transport. A request with no [`ListenerScheme`] extension counts as
/// plaintext, matching how the broker treats an unlabelled request elsewhere.
pub(crate) async fn set_strict_transport_security(State(policy): State<StrictTransportSecurity>, request: Request, next: Next) -> Response {
    let over_tls = request
        .extensions()
        .get::<ListenerScheme>()
        .is_some_and(|scheme| ApiRootScheme::from(*scheme) == ApiRootScheme::Https);
    let mut response = next.run(request).await;
    if over_tls {
        response.headers_mut().typed_insert(policy);
    }
    response
}

#[cfg(test)]
mod tests {
    use crate::stack::strict_transport_security_layer::{set_strict_transport_security, strict_transport_security};
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::{
            feature_mode::FeatureMode,
            http::security_headers::{SecurityHeadersConfig, strict_transport_security::StrictTransportSecurityConfig},
        },
    };
    use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        middleware,
        routing::get,
    };
    use headers::StrictTransportSecurity;
    use std::error::Error;
    use tower::ServiceExt;

    /// A configuration with the field turned on for 180 days.
    fn turned_on(include_subdomains: FeatureMode) -> SecurityHeadersConfig {
        SecurityHeadersConfig {
            strict_transport_security: StrictTransportSecurityConfig {
                mode: FeatureMode::Enabled,
                max_age: HumanDuration::from_days(180),
                include_subdomains,
            },
            ..SecurityHeadersConfig::default()
        }
    }

    /// The `Strict-Transport-Security` value a response to `/` carries when the
    /// request arrived with `scheme` on its listener, or with no listener scheme.
    async fn policy_header_for(scheme: Option<ListenerScheme>) -> Result<Option<String>, Box<dyn Error>> {
        let router = Router::new().route("/", get(|| async { StatusCode::OK })).layer(middleware::from_fn_with_state(
            StrictTransportSecurity::excluding_subdomains(HumanDuration::from_days(180).duration()),
            set_strict_transport_security,
        ));
        let mut request = Request::get("/").body(Body::empty())?;
        if let Some(scheme) = scheme {
            request.extensions_mut().insert(scheme);
        }
        let response = router.oneshot(request).await?;
        Ok(response
            .headers()
            .get(header::STRICT_TRANSPORT_SECURITY)
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()?)
    }

    #[test]
    fn the_default_configuration_sends_no_policy() {
        pretty_assertions::assert_eq!(strict_transport_security(&SecurityHeadersConfig::default()), None);
    }

    #[test]
    fn a_policy_is_not_sent_while_the_security_fields_are_off() {
        let config = SecurityHeadersConfig {
            mode: FeatureMode::Disabled,
            ..turned_on(FeatureMode::Disabled)
        };
        pretty_assertions::assert_eq!(strict_transport_security(&config), None);
    }

    #[test]
    fn a_turned_on_policy_covers_subdomains_only_when_set() {
        let max_age = HumanDuration::from_days(180).duration();
        pretty_assertions::assert_eq!(
            strict_transport_security(&turned_on(FeatureMode::Disabled)),
            Some(StrictTransportSecurity::excluding_subdomains(max_age))
        );
        pretty_assertions::assert_eq!(
            strict_transport_security(&turned_on(FeatureMode::Enabled)),
            Some(StrictTransportSecurity::including_subdomains(max_age))
        );
    }

    #[tokio::test]
    async fn a_request_over_an_https_listener_gets_the_policy() -> Result<(), Box<dyn Error>> {
        let header = policy_header_for(Some(ListenerScheme::new(ApiRootScheme::Https))).await?;
        pretty_assertions::assert_eq!(header.as_deref(), Some("max-age=15552000"));
        Ok(())
    }

    #[tokio::test]
    async fn a_request_over_a_plaintext_listener_gets_no_policy() -> Result<(), Box<dyn Error>> {
        let header = policy_header_for(Some(ListenerScheme::new(ApiRootScheme::Http))).await?;
        pretty_assertions::assert_eq!(header, None);
        Ok(())
    }

    #[tokio::test]
    async fn a_request_without_a_listener_scheme_gets_no_policy() -> Result<(), Box<dyn Error>> {
        let header = policy_header_for(None).await?;
        pretty_assertions::assert_eq!(header, None);
        Ok(())
    }
}
