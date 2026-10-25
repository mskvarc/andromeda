//! The `Strict-Transport-Security` response header, sent only over TLS.

use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
use axum::{
    extract::Request,
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

/// Max-age of 180 days (15552000 seconds, RFC 6797 section 6.1.1), applied to
/// subdomains as well.
const STRICT_TRANSPORT_SECURITY_POLICY: HeaderValue = HeaderValue::from_static("max-age=15552000; includeSubDomains");

/// Sets `Strict-Transport-Security` on a response to a request that arrived over
/// an `https` listener, on every such response including errors.
///
/// A request with no [`ListenerScheme`] extension counts as plaintext, matching
/// how the broker treats an unlabelled request elsewhere.
pub(crate) async fn set_strict_transport_security(request: Request, next: Next) -> Response {
    let over_tls = request
        .extensions()
        .get::<ListenerScheme>()
        .is_some_and(|scheme| ApiRootScheme::from(*scheme) == ApiRootScheme::Https);
    let mut response = next.run(request).await;
    if over_tls {
        response
            .headers_mut()
            .insert(header::STRICT_TRANSPORT_SECURITY, STRICT_TRANSPORT_SECURITY_POLICY);
    }
    response
}

#[cfg(test)]
mod tests {
    use crate::stack::strict_transport_security_layer::set_strict_transport_security;
    use andromeda_http_headers::{api_root_scheme::ApiRootScheme, listener_scheme::ListenerScheme};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        middleware,
        routing::get,
    };
    use std::error::Error;
    use tower::ServiceExt;

    /// The `Strict-Transport-Security` value a response to `/` carries when the
    /// request arrived with `scheme` on its listener, or with no listener scheme.
    async fn policy_header_for(scheme: Option<ListenerScheme>) -> Result<Option<String>, Box<dyn Error>> {
        let router = Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(set_strict_transport_security));
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

    #[tokio::test]
    async fn a_request_over_an_https_listener_gets_the_policy() -> Result<(), Box<dyn Error>> {
        let header = policy_header_for(Some(ListenerScheme::new(ApiRootScheme::Https))).await?;
        pretty_assertions::assert_eq!(header.as_deref(), Some("max-age=15552000; includeSubDomains"));
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
