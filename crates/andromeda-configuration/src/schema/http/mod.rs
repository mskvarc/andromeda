//! The `[http]` table: how the inbound HTTP stack treats a request it accepts.
//!
//! What a request may cost is `[request_limits]`; this table holds the edge
//! features, each a table of its own. An on/off feature is an
//! [`FeatureMode`](crate::schema::feature_mode::FeatureMode), and an
//! enumerable set of independent choices, the content codings, is a typed
//! [`EnumSet`](enumset::EnumSet) listed as a token array.

pub mod body_decoding;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod decompression;
pub mod security_headers;
pub mod server_timing;

use crate::schema::{
    forwarded_header_trust::ForwardedHeaderTrust,
    http::{
        body_decoding::BodyDecodingConfig,
        compression::CompressionConfig,
        cors::CorsConfig,
        csrf::CsrfConfig,
        decompression::DecompressionConfig,
        security_headers::SecurityHeadersConfig,
        server_timing::ServerTimingConfig,
    },
    link_field_layout::LinkFieldLayout,
};
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// How the inbound HTTP stack treats a request it accepts.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HttpStackConfig {
    /// Whether the `Forwarded` and `X-Forwarded-Proto` fields of an inbound
    /// request may decide the scheme of the URLs the broker mints from the
    /// request origin: `untrusted` or `trusted`.
    ///
    /// Untrusted by default. Both fields are ordinary request fields, so
    /// believing them without a reverse proxy that overwrites them lets any
    /// client choose the scheme of a `Location` field, a pagination `Link`, and
    /// the durably stored `@context` URL of clause 5.13. Trust them only
    /// behind a reverse proxy that overwrites both fields.
    #[toml_comment(inline)]
    pub forwarded_headers: ForwardedHeaderTrust,
    /// How the link-values of a response or an HTTP notification are laid out
    /// over `Link` field lines: `combined` (one line), `grouped` (the
    /// `@context` link and the pagination links on a line each) or `separate`
    /// (one line per link-value).
    ///
    /// Every layout means the same (IETF RFC 9110 section 5.3); they differ in
    /// which clients read them. `combined` suits clients that read only one
    /// field line, `separate` clients that never split a line on commas.
    #[toml_comment(inline)]
    pub link_field: LinkFieldLayout,
    /// Where request payloads are decoded.
    pub body_decoding: BodyDecodingConfig,
    /// CORS policy for browser-facing clients.
    pub cors: CorsConfig,
    /// Response compression.
    pub compression: CompressionConfig,
    /// Request decompression.
    pub decompression: DecompressionConfig,
    /// The `Server-Timing` response field.
    pub server_timing: ServerTimingConfig,
    /// The security response fields.
    pub security_headers: SecurityHeadersConfig,
    /// Cross-Site Request Forgery protection.
    pub csrf: CsrfConfig,
}

#[cfg(test)]
mod tests {
    use crate::schema::{forwarded_header_trust::ForwardedHeaderTrust, http::HttpStackConfig, link_field_layout::LinkFieldLayout};
    use toml_comment::TomlComment;

    /// A broker that has not been told about a proxy must not believe one, so
    /// the declared default has to be the untrusted edge.
    #[test]
    fn forwarded_fields_are_untrusted_in_the_default_configuration() {
        pretty_assertions::assert_eq!(HttpStackConfig::default().forwarded_headers, ForwardedHeaderTrust::Untrusted);
    }

    /// One `Link` field line is what every client reads, so it is the default.
    #[test]
    fn link_fields_are_combined_in_the_default_configuration() {
        pretty_assertions::assert_eq!(HttpStackConfig::default().link_field, LinkFieldLayout::Combined);
    }

    #[test]
    fn default_toml_contains_every_nested_http_table() {
        let rendered = <HttpStackConfig as TomlComment>::default_toml();
        for table in [
            "[body_decoding]",
            "[cors]",
            "[compression]",
            "[decompression]",
            "[server_timing]",
            "[security_headers]",
            "[csrf]",
        ] {
            assert!(rendered.contains(table), "missing {table} in:\n{rendered}");
        }
    }
}
