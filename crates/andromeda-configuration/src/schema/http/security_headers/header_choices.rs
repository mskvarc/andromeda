//! The values a security response field with a fixed vocabulary can take.
//!
//! Each choice names one field value, spelled as the field spells it in
//! kebab-case, plus `disabled`, which leaves the field out of the response. The
//! HTTP layer maps each choice to the value it sends.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The `Cross-Origin-Opener-Policy` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `Cross-Origin-Opener-Policy` header.")]
pub enum CrossOriginOpenerPolicySetting {
    /// `same-origin`.
    #[default]
    #[schemars(description = "Sends `same-origin`.")]
    SameOrigin,
    /// `same-origin-allow-popups`.
    #[schemars(description = "Sends `same-origin-allow-popups`.")]
    SameOriginAllowPopups,
    /// `unsafe-none`.
    #[schemars(description = "Sends `unsafe-none`.")]
    UnsafeNone,
    /// No field.
    #[schemars(description = "Sends no `Cross-Origin-Opener-Policy` header.")]
    Disabled,
}

/// The `Cross-Origin-Resource-Policy` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `Cross-Origin-Resource-Policy` header.")]
pub enum CrossOriginResourcePolicySetting {
    /// `same-origin`.
    #[default]
    #[schemars(description = "Sends `same-origin`.")]
    SameOrigin,
    /// `same-site`.
    #[schemars(description = "Sends `same-site`.")]
    SameSite,
    /// `cross-origin`.
    #[schemars(description = "Sends `cross-origin`.")]
    CrossOrigin,
    /// No field.
    #[schemars(description = "Sends no `Cross-Origin-Resource-Policy` header.")]
    Disabled,
}

/// The `Origin-Agent-Cluster` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `Origin-Agent-Cluster` header.")]
pub enum OriginAgentClusterSetting {
    /// `?1`: the browser keys agent clusters by origin.
    #[default]
    #[schemars(description = "Sends `?1`, asking the browser to isolate the broker's origin.")]
    OriginKeyed,
    /// `?0`: the browser keys agent clusters by site.
    #[schemars(description = "Sends `?0`, letting the browser share an agent cluster across the site.")]
    SiteKeyed,
    /// No field.
    #[schemars(description = "Sends no `Origin-Agent-Cluster` header.")]
    Disabled,
}

/// The `Referrer-Policy` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `Referrer-Policy` header.")]
pub enum ReferrerPolicySetting {
    /// `no-referrer`.
    #[default]
    #[schemars(description = "Sends `no-referrer`.")]
    NoReferrer,
    /// `no-referrer-when-downgrade`.
    #[schemars(description = "Sends `no-referrer-when-downgrade`.")]
    NoReferrerWhenDowngrade,
    /// `origin`.
    #[schemars(description = "Sends `origin`.")]
    Origin,
    /// `origin-when-cross-origin`.
    #[schemars(description = "Sends `origin-when-cross-origin`.")]
    OriginWhenCrossOrigin,
    /// `same-origin`.
    #[schemars(description = "Sends `same-origin`.")]
    SameOrigin,
    /// `strict-origin`.
    #[schemars(description = "Sends `strict-origin`.")]
    StrictOrigin,
    /// `strict-origin-when-cross-origin`.
    #[schemars(description = "Sends `strict-origin-when-cross-origin`.")]
    StrictOriginWhenCrossOrigin,
    /// `unsafe-url`.
    #[schemars(description = "Sends `unsafe-url`.")]
    UnsafeUrl,
    /// No field.
    #[schemars(description = "Sends no `Referrer-Policy` header.")]
    Disabled,
}

/// The `X-Content-Type-Options` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-Content-Type-Options` header.")]
pub enum XContentTypeOptionsSetting {
    /// `nosniff`.
    #[default]
    #[schemars(description = "Sends `nosniff`.")]
    Nosniff,
    /// No field.
    #[schemars(description = "Sends no `X-Content-Type-Options` header.")]
    Disabled,
}

/// The `X-DNS-Prefetch-Control` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-DNS-Prefetch-Control` header.")]
pub enum XDnsPrefetchControlSetting {
    /// `off`: the field is sent and asks the browser not to prefetch.
    #[default]
    #[schemars(description = "Sends `off`.")]
    Off,
    /// `on`.
    #[schemars(description = "Sends `on`.")]
    On,
    /// No field.
    #[schemars(description = "Sends no `X-DNS-Prefetch-Control` header.")]
    Disabled,
}

/// The `X-Download-Options` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-Download-Options` header.")]
pub enum XDownloadOptionsSetting {
    /// `noopen`.
    #[default]
    #[schemars(description = "Sends `noopen`.")]
    Noopen,
    /// No field.
    #[schemars(description = "Sends no `X-Download-Options` header.")]
    Disabled,
}

/// The `X-Frame-Options` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-Frame-Options` header.")]
pub enum XFrameOptionsSetting {
    /// `SAMEORIGIN`.
    #[default]
    #[schemars(description = "Sends `SAMEORIGIN`.")]
    SameOrigin,
    /// `DENY`.
    #[schemars(description = "Sends `DENY`.")]
    Deny,
    /// No field.
    #[schemars(description = "Sends no `X-Frame-Options` header.")]
    Disabled,
}

/// The `X-Permitted-Cross-Domain-Policies` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-Permitted-Cross-Domain-Policies` header.")]
pub enum XPermittedCrossDomainPoliciesSetting {
    /// `none`.
    #[default]
    #[schemars(description = "Sends `none`.")]
    None,
    /// `master-only`.
    #[schemars(description = "Sends `master-only`.")]
    MasterOnly,
    /// `by-content-type`.
    #[schemars(description = "Sends `by-content-type`.")]
    ByContentType,
    /// `by-ftp-filename`.
    #[schemars(description = "Sends `by-ftp-filename`.")]
    ByFtpFilename,
    /// `all`.
    #[schemars(description = "Sends `all`.")]
    All,
    /// No field.
    #[schemars(description = "Sends no `X-Permitted-Cross-Domain-Policies` header.")]
    Disabled,
}

/// The `X-XSS-Protection` field.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The `X-XSS-Protection` header.")]
pub enum XXssProtectionSetting {
    /// `0`: the field is sent and turns the browser's filter off.
    #[default]
    #[schemars(description = "Sends `0`, turning a browser's XSS filter off.")]
    Off,
    /// `1`.
    #[schemars(description = "Sends `1`.")]
    On,
    /// `1; mode=block`.
    #[schemars(description = "Sends `1; mode=block`.")]
    Block,
    /// No field.
    #[schemars(description = "Sends no `X-XSS-Protection` header.")]
    Disabled,
}

#[cfg(test)]
mod tests {
    use crate::schema::http::security_headers::header_choices::{OriginAgentClusterSetting, XPermittedCrossDomainPoliciesSetting, XXssProtectionSetting};
    use std::str::FromStr;
    use strum::ParseError;

    #[test]
    fn a_choice_reads_from_its_kebab_case_token() -> Result<(), ParseError> {
        pretty_assertions::assert_eq!(OriginAgentClusterSetting::from_str("site-keyed")?, OriginAgentClusterSetting::SiteKeyed);
        pretty_assertions::assert_eq!(
            XPermittedCrossDomainPoliciesSetting::from_str("none")?,
            XPermittedCrossDomainPoliciesSetting::None
        );
        pretty_assertions::assert_eq!(XXssProtectionSetting::from_str("disabled")?, XXssProtectionSetting::Disabled);
        Ok(())
    }

    #[test]
    fn an_unknown_token_is_refused() {
        assert!(XXssProtectionSetting::from_str("sometimes").is_err());
    }
}
