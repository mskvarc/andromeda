//! The security-header (helmet) middleware layer the configured fields render
//! into.
//!
//! The configuration names each field's value; the values a field sends are
//! the HTTP layer's, so each choice is mapped here. `Strict-Transport-Security`
//! is not part of this layer: it depends on the listener a request arrived on.

use andromeda_configuration::schema::{
    feature_mode::FeatureMode,
    http::security_headers::{
        SecurityHeadersConfig,
        content_security_policy::ContentSecurityPolicyConfig,
        header_choices::{
            CrossOriginOpenerPolicySetting,
            CrossOriginResourcePolicySetting,
            OriginAgentClusterSetting,
            ReferrerPolicySetting,
            XContentTypeOptionsSetting,
            XDnsPrefetchControlSetting,
            XDownloadOptionsSetting,
            XFrameOptionsSetting,
            XPermittedCrossDomainPoliciesSetting,
            XXssProtectionSetting,
        },
    },
};
use axum_helmet::{
    CrossOriginOpenerPolicy,
    CrossOriginResourcePolicy,
    Header,
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
use thiserror::Error;

/// The configured security fields hold a value no response field can carry.
#[derive(Debug, Error)]
#[error("http.security_headers holds a value no response header can carry: {0}")]
pub struct SecurityHeadersError(#[from] HelmetError);

/// Render every configured field, so a policy no field can carry is refused
/// while the configuration loads.
///
/// # Errors
///
/// Returns [`SecurityHeadersError`] when a configured value is not a field
/// value.
pub fn check_security_headers(config: &SecurityHeadersConfig) -> Result<(), SecurityHeadersError> {
    build_helmet_layer(config).map(drop)
}

/// Build the security-header layer, or `None` when the policy disables it.
///
/// `axum-helmet` renders every field value up front, so building the layer
/// once at startup surfaces an invalid value before the broker serves traffic.
///
/// # Errors
///
/// Returns [`SecurityHeadersError`] when a configured value is not a field
/// value.
pub(crate) fn build_helmet_layer(config: &SecurityHeadersConfig) -> Result<Option<HelmetLayer>, SecurityHeadersError> {
    match config.mode {
        FeatureMode::Disabled => Ok(None),
        FeatureMode::Enabled => Ok(Some(security_header_policy(config).try_into()?)),
    }
}

/// The fields `config` sends, without `Strict-Transport-Security`.
fn security_header_policy(config: &SecurityHeadersConfig) -> Helmet {
    [
        content_security_policy(&config.content_security_policy),
        cross_origin_opener_policy(config.cross_origin_opener_policy),
        cross_origin_resource_policy(config.cross_origin_resource_policy),
        origin_agent_cluster(config.origin_agent_cluster),
        referrer_policy(config.referrer_policy),
        x_content_type_options(config.x_content_type_options),
        x_dns_prefetch_control(config.x_dns_prefetch_control),
        x_download_options(config.x_download_options),
        x_frame_options(config.x_frame_options),
        x_permitted_cross_domain_policies(config.x_permitted_cross_domain_policies),
        x_xss_protection(config.x_xss_protection),
    ]
    .into_iter()
    .flatten()
    .fold(Helmet::new(), Helmet::add)
}

/// The `Content-Security-Policy` field, carrying the policy as written.
fn content_security_policy(config: &ContentSecurityPolicyConfig) -> Option<Header> {
    match config.mode {
        FeatureMode::Enabled => Some(("Content-Security-Policy", config.policy.as_str().to_owned())),
        FeatureMode::Disabled => None,
    }
}

/// The `Cross-Origin-Opener-Policy` field.
fn cross_origin_opener_policy(setting: CrossOriginOpenerPolicySetting) -> Option<Header> {
    match setting {
        CrossOriginOpenerPolicySetting::SameOrigin => Some(CrossOriginOpenerPolicy::same_origin().into()),
        CrossOriginOpenerPolicySetting::SameOriginAllowPopups => Some(CrossOriginOpenerPolicy::same_origin_allow_popups().into()),
        CrossOriginOpenerPolicySetting::UnsafeNone => Some(CrossOriginOpenerPolicy::unsafe_none().into()),
        CrossOriginOpenerPolicySetting::Disabled => None,
    }
}

/// The `Cross-Origin-Resource-Policy` field.
fn cross_origin_resource_policy(setting: CrossOriginResourcePolicySetting) -> Option<Header> {
    match setting {
        CrossOriginResourcePolicySetting::SameOrigin => Some(CrossOriginResourcePolicy::same_origin().into()),
        CrossOriginResourcePolicySetting::SameSite => Some(CrossOriginResourcePolicy::same_site().into()),
        CrossOriginResourcePolicySetting::CrossOrigin => Some(CrossOriginResourcePolicy::cross_origin().into()),
        CrossOriginResourcePolicySetting::Disabled => None,
    }
}

/// The `Origin-Agent-Cluster` field.
fn origin_agent_cluster(setting: OriginAgentClusterSetting) -> Option<Header> {
    match setting {
        OriginAgentClusterSetting::OriginKeyed => Some(OriginAgentCluster::new(true).into()),
        OriginAgentClusterSetting::SiteKeyed => Some(OriginAgentCluster::new(false).into()),
        OriginAgentClusterSetting::Disabled => None,
    }
}

/// The `Referrer-Policy` field.
fn referrer_policy(setting: ReferrerPolicySetting) -> Option<Header> {
    let policy = match setting {
        ReferrerPolicySetting::NoReferrer => ReferrerPolicy::no_referrer(),
        ReferrerPolicySetting::NoReferrerWhenDowngrade => ReferrerPolicy::no_referrer_when_downgrade(),
        ReferrerPolicySetting::Origin => ReferrerPolicy::origin(),
        ReferrerPolicySetting::OriginWhenCrossOrigin => ReferrerPolicy::origin_when_cross_origin(),
        ReferrerPolicySetting::SameOrigin => ReferrerPolicy::same_origin(),
        ReferrerPolicySetting::StrictOrigin => ReferrerPolicy::strict_origin(),
        ReferrerPolicySetting::StrictOriginWhenCrossOrigin => ReferrerPolicy::strict_origin_when_cross_origin(),
        ReferrerPolicySetting::UnsafeUrl => ReferrerPolicy::unsafe_url(),
        ReferrerPolicySetting::Disabled => return None,
    };
    Some(policy.into())
}

/// The `X-Content-Type-Options` field.
fn x_content_type_options(setting: XContentTypeOptionsSetting) -> Option<Header> {
    match setting {
        XContentTypeOptionsSetting::Nosniff => Some(XContentTypeOptions::nosniff().into()),
        XContentTypeOptionsSetting::Disabled => None,
    }
}

/// The `X-DNS-Prefetch-Control` field.
fn x_dns_prefetch_control(setting: XDnsPrefetchControlSetting) -> Option<Header> {
    match setting {
        XDnsPrefetchControlSetting::Off => Some(XDNSPrefetchControl::off().into()),
        XDnsPrefetchControlSetting::On => Some(XDNSPrefetchControl::on().into()),
        XDnsPrefetchControlSetting::Disabled => None,
    }
}

/// The `X-Download-Options` field.
fn x_download_options(setting: XDownloadOptionsSetting) -> Option<Header> {
    match setting {
        XDownloadOptionsSetting::Noopen => Some(XDownloadOptions::noopen().into()),
        XDownloadOptionsSetting::Disabled => None,
    }
}

/// The `X-Frame-Options` field.
fn x_frame_options(setting: XFrameOptionsSetting) -> Option<Header> {
    match setting {
        XFrameOptionsSetting::SameOrigin => Some(XFrameOptions::same_origin().into()),
        XFrameOptionsSetting::Deny => Some(XFrameOptions::deny().into()),
        XFrameOptionsSetting::Disabled => None,
    }
}

/// The `X-Permitted-Cross-Domain-Policies` field.
fn x_permitted_cross_domain_policies(setting: XPermittedCrossDomainPoliciesSetting) -> Option<Header> {
    let policies = match setting {
        XPermittedCrossDomainPoliciesSetting::None => XPermittedCrossDomainPolicies::none(),
        XPermittedCrossDomainPoliciesSetting::MasterOnly => XPermittedCrossDomainPolicies::master_only(),
        XPermittedCrossDomainPoliciesSetting::ByContentType => XPermittedCrossDomainPolicies::by_content_type(),
        XPermittedCrossDomainPoliciesSetting::ByFtpFilename => XPermittedCrossDomainPolicies::by_ftp_filename(),
        XPermittedCrossDomainPoliciesSetting::All => XPermittedCrossDomainPolicies::all(),
        XPermittedCrossDomainPoliciesSetting::Disabled => return None,
    };
    Some(policies.into())
}

/// The `X-XSS-Protection` field.
fn x_xss_protection(setting: XXssProtectionSetting) -> Option<Header> {
    match setting {
        XXssProtectionSetting::Off => Some(XXSSProtection::off().into()),
        XXssProtectionSetting::On => Some(XXSSProtection::on().into()),
        XXssProtectionSetting::Block => Some(XXSSProtection::on().mode_block().into()),
        XXssProtectionSetting::Disabled => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::stack::helmet_layer::{build_helmet_layer, check_security_headers};
    use andromeda_configuration::schema::{
        feature_mode::FeatureMode,
        http::security_headers::{
            SecurityHeadersConfig,
            content_security_policy::ContentSecurityPolicyConfig,
            header_choices::{ReferrerPolicySetting, XFrameOptionsSetting},
        },
    };
    use axum::{
        Router,
        body::Body,
        http::{HeaderMap, HeaderValue, Request, StatusCode, header},
        routing::get,
    };
    use axum_helmet::ContentSecurityPolicy;
    use std::error::Error;
    use tower::ServiceExt;

    /// The fields the layer adds to a response to `/` under `config`, without
    /// the `Content-Length` the router sets.
    async fn headers_sent_under(config: &SecurityHeadersConfig) -> Result<HeaderMap, Box<dyn Error>> {
        let layer = build_helmet_layer(config)?.ok_or("an enabled policy builds a layer")?;
        let router = Router::new().route("/", get(|| async { StatusCode::OK })).layer(layer);
        let mut headers = router.oneshot(Request::get("/").body(Body::empty())?).await?.into_parts().0.headers;
        headers.remove(header::CONTENT_LENGTH);
        Ok(headers)
    }

    #[test]
    fn a_disabled_policy_builds_no_layer() -> Result<(), Box<dyn Error>> {
        let config = SecurityHeadersConfig {
            mode: FeatureMode::Disabled,
            ..SecurityHeadersConfig::default()
        };
        assert!(build_helmet_layer(&config)?.is_none());
        Ok(())
    }

    #[test]
    fn the_default_policy_is_the_helmet_default() {
        pretty_assertions::assert_eq!(
            ContentSecurityPolicyConfig::default().policy.as_str(),
            ContentSecurityPolicy::default().to_string()
        );
    }

    #[tokio::test]
    async fn the_default_policy_renders_the_header_set_without_strict_transport_security() -> Result<(), Box<dyn Error>> {
        let headers = headers_sent_under(&SecurityHeadersConfig::default()).await?;
        let mut sent: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| Ok((name.as_str(), value.to_str()?)))
            .collect::<Result<_, Box<dyn Error>>>()?;
        sent.sort_unstable();
        let sent = sent;
        let policy = ContentSecurityPolicy::default().to_string();
        pretty_assertions::assert_eq!(
            sent,
            [
                ("content-security-policy", policy.as_str()),
                ("cross-origin-opener-policy", "same-origin"),
                ("cross-origin-resource-policy", "same-origin"),
                ("origin-agent-cluster", "?1"),
                ("referrer-policy", "no-referrer"),
                ("x-content-type-options", "nosniff"),
                ("x-dns-prefetch-control", "off"),
                ("x-download-options", "noopen"),
                ("x-frame-options", "SAMEORIGIN"),
                ("x-permitted-cross-domain-policies", "none"),
                ("x-xss-protection", "0"),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn one_header_is_changed_and_another_turned_off_while_the_others_stay() -> Result<(), Box<dyn Error>> {
        let config = SecurityHeadersConfig {
            referrer_policy: ReferrerPolicySetting::StrictOriginWhenCrossOrigin,
            x_frame_options: XFrameOptionsSetting::Disabled,
            ..SecurityHeadersConfig::default()
        };
        let headers = headers_sent_under(&config).await?;
        pretty_assertions::assert_eq!(
            headers.get(header::REFERRER_POLICY).map(HeaderValue::as_bytes),
            Some(&b"strict-origin-when-cross-origin"[..])
        );
        assert!(
            !headers.contains_key(header::X_FRAME_OPTIONS),
            "X-Frame-Options was turned off, found {headers:?}"
        );
        pretty_assertions::assert_eq!(headers.get(header::X_CONTENT_TYPE_OPTIONS).map(HeaderValue::as_bytes), Some(&b"nosniff"[..]));
        pretty_assertions::assert_eq!(headers.len(), 10);
        Ok(())
    }

    #[tokio::test]
    async fn a_disabled_content_security_policy_leaves_the_field_out() -> Result<(), Box<dyn Error>> {
        let config = SecurityHeadersConfig {
            content_security_policy: ContentSecurityPolicyConfig {
                mode: FeatureMode::Disabled,
                ..ContentSecurityPolicyConfig::default()
            },
            ..SecurityHeadersConfig::default()
        };
        let headers = headers_sent_under(&config).await?;
        assert!(!headers.contains_key(header::CONTENT_SECURITY_POLICY), "found {headers:?}");
        Ok(())
    }

    #[test]
    fn a_policy_with_a_line_break_is_refused() -> Result<(), Box<dyn Error>> {
        let config = SecurityHeadersConfig {
            content_security_policy: toml::from_str("mode = \"enabled\"\npolicy = \"default-src 'self'\\nscript-src 'none'\"")?,
            ..SecurityHeadersConfig::default()
        };
        assert!(check_security_headers(&config).is_err());
        Ok(())
    }
}
