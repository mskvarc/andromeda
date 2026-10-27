//! Construction of the security-header (helmet) middleware layer.

use andromeda_configuration::schema::{feature_mode::FeatureMode, http::security_headers::SecurityHeadersConfig};
use axum_helmet::{
    ContentSecurityPolicy,
    CrossOriginOpenerPolicy,
    CrossOriginResourcePolicy,
    Helmet,
    HelmetError,
    HelmetLayer,
    OriginAgentCluster,
    ReferrerPolicy,
    XContentTypeOptions,
    XDNSPrefetchControl,
    XDownloadOptions,
    XFrameOptions,
    XPermittedCrossDomainPolicies,
    XXSSProtection,
};

/// Build the security-header layer, or `None` when the policy disables it.
///
/// The layer is fallible because `axum-helmet` renders every header value up
/// front and reports an invalid one as [`HelmetError`]; building it once at
/// startup surfaces that failure before the broker serves traffic.
///
/// # Errors
///
/// Returns [`HelmetError`] when the default header set cannot be rendered into a
/// layer.
pub(crate) fn build_helmet_layer(config: SecurityHeadersConfig) -> Result<Option<HelmetLayer>, HelmetError> {
    match config.mode {
        FeatureMode::Disabled => Ok(None),
        FeatureMode::Enabled => security_header_policy().try_into().map(Some),
    }
}

/// The `axum-helmet` default header set without `Strict-Transport-Security`.
///
/// RFC 6797 section 7.2 has a user agent ignore that header on a plaintext
/// transport, so the broker adds it per response, and only for a TLS listener.
fn security_header_policy() -> Helmet {
    Helmet::new()
        .add(ContentSecurityPolicy::default())
        .add(CrossOriginOpenerPolicy::same_origin())
        .add(CrossOriginResourcePolicy::same_origin())
        .add(OriginAgentCluster::new(true))
        .add(ReferrerPolicy::no_referrer())
        .add(XContentTypeOptions::nosniff())
        .add(XDNSPrefetchControl::off())
        .add(XDownloadOptions::noopen())
        .add(XFrameOptions::same_origin())
        .add(XPermittedCrossDomainPolicies::none())
        .add(XXSSProtection::off())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stack::helmet_layer::build_helmet_layer;
    use andromeda_configuration::schema::{feature_mode::FeatureMode, http::security_headers::SecurityHeadersConfig};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        routing::get,
    };
    use std::error::Error;
    use tower::ServiceExt;

    #[test]
    fn an_enabled_policy_builds_a_layer() {
        let layer = build_helmet_layer(SecurityHeadersConfig { mode: FeatureMode::Enabled }).expect("default helmet builds");
        assert!(layer.is_some());
    }

    #[test]
    fn a_disabled_policy_builds_no_layer() {
        let layer = build_helmet_layer(SecurityHeadersConfig { mode: FeatureMode::Disabled }).expect("disabled helmet builds");
        assert!(layer.is_none());
    }

    #[tokio::test]
    async fn the_enabled_policy_sets_its_headers_but_not_strict_transport_security() -> Result<(), Box<dyn Error>> {
        let layer = build_helmet_layer(SecurityHeadersConfig { mode: FeatureMode::Enabled })?.ok_or("an enabled policy builds a layer")?;
        let router = Router::new().route("/", get(|| async { StatusCode::OK })).layer(layer);
        let response = router.oneshot(Request::get("/").body(Body::empty())?).await?;
        assert!(response.headers().contains_key(header::X_CONTENT_TYPE_OPTIONS));
        assert!(
            !response.headers().contains_key(header::STRICT_TRANSPORT_SECURITY),
            "the policy must leave Strict-Transport-Security to the listener, found {:?}",
            response.headers()
        );
        Ok(())
    }
}
