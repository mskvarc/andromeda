//! The well-known `contextSourceInfo` keys (ETSI GS CIM 009 clause 4.3.6.6).
//!
//! A registration's `contextSourceInfo` is a free-form [`KeyValuePair`] array
//! that is forwarded as headers when the registered source is contacted
//! (clause 6.3.19). Four keys are reserved: clause 4.3.6.6 gives them a fixed
//! meaning and a restriction on their value, and clause 5.9.2.4 makes a
//! violation of those restrictions a `BadRequestData` error at registration
//! time. Every other pair becomes an HTTP header field, so a key or value that
//! breaks IETF RFC 7230 is refused when the registration is written rather than
//! when a request is forwarded. This module is the one reading of the array both
//! the registration policy (validation) and forwarding (application) share.

use crate::{
    http_header_field::{HttpHeaderField, HttpHeaderFieldError},
    key_value_pair::KeyValuePair,
};
use andromeda_ngsi_ld_lexicon::spec_version::{SpecVersion, SpecVersionParseError};
use iri_rs::{InvalidIri, IriBuf};
use mediatype::{
    MediaType,
    MediaTypeBuf,
    MediaTypeError,
    Name,
    names::{APPLICATION, JSON},
};
use std::str::FromStr;
use strum::{Display, IntoStaticStr};
use thiserror::Error;

/// The `accept` key: the media type the source must answer in.
pub const ACCEPT_KEY: &str = "accept";
/// The `contentType` key: the media type a forwarded payload is sent as.
pub const CONTENT_TYPE_KEY: &str = "contentType";
/// The `jsonldContext` key: the context a forwarded request is compacted against.
pub const JSONLD_CONTEXT_KEY: &str = "jsonldContext";
/// The `ngsildConformance` key: the specification version the source understands.
pub const NGSILD_CONFORMANCE_KEY: &str = "ngsildConformance";

/// The two reserved keys whose value clause 4.3.6.6 restricts to a media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
pub enum MediaTypeKey {
    /// The `accept` key.
    #[strum(serialize = "accept")]
    Accept,
    /// The `contentType` key.
    #[strum(serialize = "contentType")]
    ContentType,
}

/// Why a `contextSourceInfo` array violates clause 4.3.6.6.
#[derive(Debug, Error)]
pub enum ContextSourceInfoError {
    /// `accept` or `contentType` is not a media type at all.
    #[error("contextSourceInfo {key} is not a media type: {source}")]
    MalformedMediaType {
        /// Which of the two media-type keys carried the value.
        key: MediaTypeKey,
        /// Why the media-type grammar refused the value.
        #[source]
        source: MediaTypeError,
    },
    /// `accept` or `contentType` is a media type, but not one of the two the clause allows.
    #[error("contextSourceInfo {key} must be application/json or application/ld+json, got {media_type}")]
    UnsupportedMediaType {
        /// Which of the two media-type keys carried the value.
        key: MediaTypeKey,
        /// The media type the key named.
        media_type: MediaTypeBuf,
    },
    /// `jsonldContext` is not an absolute URL: a registration has no base a
    /// relative reference could be resolved against, so nothing could be
    /// dereferenced from one.
    #[error("contextSourceInfo jsonldContext must be an absolute URL")]
    InvalidJsonLdContext(#[source] InvalidIri<String>),
    /// `ngsildConformance` is not of the form `major.minor`.
    #[error("contextSourceInfo ngsildConformance must be of the form major.minor")]
    InvalidConformance(#[source] SpecVersionParseError),
    /// A pair outside the reserved keys is not an HTTP header field (clause 6.3.19).
    #[error("contextSourceInfo holds an invalid HTTP header: {0}")]
    CustomHeader(#[source] HttpHeaderFieldError),
}

/// The reserved keys of a `contextSourceInfo` array, read into their types, and
/// the remaining pairs that are forwarded verbatim (clause 6.3.19).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSourceInfo {
    /// The media type the registered source shall answer in.
    pub accept: Option<MediaTypeBuf>,
    /// The media type a forwarded payload shall be sent as.
    pub content_type: Option<MediaTypeBuf>,
    /// The JSON-LD context a forwarded request shall be compacted against.
    pub jsonld_context: Option<IriBuf>,
    /// The specification version the forwarded payload shall conform to.
    pub ngsild_conformance: Option<SpecVersion>,
    /// Every pair outside the reserved keys as an HTTP header field, in
    /// registration order. Tenant information and binding-specific headers
    /// such as `Content-Length` are kept and admitted: clause 4.3.6.5 has them
    /// ignored rather than refused, so the forwarder never sends them and the
    /// tenant comes from the registration's `tenant` member alone (clauses
    /// 4.14 and 6.3.19).
    pub custom: Vec<HttpHeaderField>,
}

/// Whether a media type is one clause 4.3.6.6 admits for `accept` and `contentType`.
fn is_admitted_media_type(media_type: &MediaType<'_>) -> bool {
    media_type.ty == APPLICATION
        && (media_type.subty == JSON && media_type.suffix.is_none() || media_type.subty == Name::new_unchecked("ld") && media_type.suffix == Some(JSON))
}

/// Parse one of the two media-type keys under the clause 4.3.6.6 restriction.
fn parse_media_type(key: MediaTypeKey, value: &str) -> Result<MediaTypeBuf, ContextSourceInfoError> {
    let parsed = MediaType::parse(value).map_err(|source| ContextSourceInfoError::MalformedMediaType { key, source })?;
    let media_type = MediaTypeBuf::from(parsed);
    if !is_admitted_media_type(&media_type.to_ref()) {
        return Err(ContextSourceInfoError::UnsupportedMediaType { key, media_type });
    }
    Ok(media_type)
}

impl TryFrom<&[KeyValuePair]> for ContextSourceInfo {
    type Error = ContextSourceInfoError;

    fn try_from(pairs: &[KeyValuePair]) -> Result<ContextSourceInfo, ContextSourceInfoError> {
        let mut info = ContextSourceInfo::default();
        for pair in pairs {
            match pair.key.as_str() {
                ACCEPT_KEY => info.accept = Some(parse_media_type(MediaTypeKey::Accept, &pair.value)?),
                CONTENT_TYPE_KEY => info.content_type = Some(parse_media_type(MediaTypeKey::ContentType, &pair.value)?),
                JSONLD_CONTEXT_KEY => {
                    info.jsonld_context = Some(IriBuf::new(pair.value.to_string()).map_err(ContextSourceInfoError::InvalidJsonLdContext)?);
                }
                NGSILD_CONFORMANCE_KEY => {
                    info.ngsild_conformance = Some(SpecVersion::from_str(&pair.value).map_err(ContextSourceInfoError::InvalidConformance)?);
                }
                _custom => info.custom.push(HttpHeaderField::try_from(pair).map_err(ContextSourceInfoError::CustomHeader)?),
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_source_info::{ContextSourceInfo, ContextSourceInfoError, MediaTypeKey},
        http_header_field::HttpHeaderFieldError,
        key_value_pair::KeyValuePair,
    };
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn the_four_reserved_keys_are_read_into_their_types_and_the_rest_kept_verbatim() {
        let pairs = [
            pair("accept", "application/ld+json"),
            pair("contentType", "application/json"),
            pair("jsonldContext", "https://example.org/context.jsonld"),
            pair("ngsildConformance", "1.6"),
            pair("Authorization", "Bearer x"),
        ];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("valid contextSourceInfo");
        pretty_assertions::assert_eq!(info.accept.as_ref().map(ToString::to_string), Some("application/ld+json".to_string()));
        pretty_assertions::assert_eq!(info.content_type.as_ref().map(ToString::to_string), Some("application/json".to_string()));
        pretty_assertions::assert_eq!(
            info.jsonld_context.as_ref().map(ToString::to_string),
            Some("https://example.org/context.jsonld".to_string())
        );
        pretty_assertions::assert_eq!(info.ngsild_conformance, Some(SpecVersion::new(1, 6)));
        pretty_assertions::assert_eq!(info.custom.len(), 1);
        pretty_assertions::assert_eq!(info.custom[0].name, "authorization");
        pretty_assertions::assert_eq!(info.custom[0].value, "Bearer x");
    }

    fn custom_error(key: &str, value: &str) -> ContextSourceInfoError {
        let pairs = [pair(key, value)];
        ContextSourceInfo::try_from(pairs.as_slice()).expect_err("the pair is refused")
    }

    #[test]
    fn a_custom_key_with_a_space_is_refused() {
        assert!(matches!(
            custom_error("Bad Name", "x"),
            ContextSourceInfoError::CustomHeader(HttpHeaderFieldError::Name(_))
        ));
    }

    #[test]
    fn a_custom_value_with_a_newline_is_refused() {
        assert!(matches!(
            custom_error("X-Api-Key", "a\nb"),
            ContextSourceInfoError::CustomHeader(HttpHeaderFieldError::Value(_))
        ));
    }

    /// Clause 4.3.6.5: tenant information "shall not be part of
    /// contextSourceInfo" and, "If present, such information shall be
    /// ignored", so the key is admitted in any casing and left for the
    /// forwarder to ignore.
    #[test]
    fn a_tenant_key_is_accepted_in_any_casing_and_left_for_the_forwarder_to_ignore() {
        for key in ["NGSILD-Tenant", "ngsild-tenant", "Ngsild-TENANT"] {
            let pairs = [pair(key, "t1")];
            let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("tenant information is ignored, not refused");
            pretty_assertions::assert_eq!(info.custom.len(), 1, "{key}");
            pretty_assertions::assert_eq!(info.custom[0].name, "ngsild-tenant", "{key}");
        }
    }

    #[test]
    fn urn_ngsi_ld_request_is_accepted_as_a_custom_value() {
        let pairs = [pair("X-Api-Key", "urn:ngsi-ld:request")];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("a placeholder value");
        pretty_assertions::assert_eq!(info.custom[0].value, "urn:ngsi-ld:request");
    }

    #[test]
    fn a_reserved_key_keeps_its_own_value_rule() {
        assert!(matches!(
            custom_error("accept", "text/plain"),
            ContextSourceInfoError::UnsupportedMediaType { key: MediaTypeKey::Accept, .. }
        ));
    }

    #[test]
    fn a_content_length_key_is_accepted_and_left_for_the_forwarder_to_ignore() {
        let pairs = [pair("Content-Length", "10")];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("connection headers are ignored, not refused");
        pretty_assertions::assert_eq!(info.custom.len(), 1);
    }

    #[test]
    fn a_media_type_outside_the_two_admitted_ones_is_rejected() {
        for (key, value, expected_key) in [
            ("accept", "text/plain", MediaTypeKey::Accept),
            ("contentType", "application/xml", MediaTypeKey::ContentType),
        ] {
            let pairs = [pair(key, value)];
            let error = ContextSourceInfo::try_from(pairs.as_slice()).expect_err("an unadmitted media type is rejected");
            let ContextSourceInfoError::UnsupportedMediaType { key: rejected_key, media_type } = error else {
                panic!("{key}: {value} must be an unsupported media type, got {error:?}");
            };
            pretty_assertions::assert_eq!(rejected_key, expected_key);
            pretty_assertions::assert_eq!(media_type.to_string(), value);
        }
    }

    #[test]
    fn a_value_that_is_not_a_media_type_names_the_key_that_carried_it() {
        let pairs = [pair("accept", "not a media type")];
        assert!(matches!(
            ContextSourceInfo::try_from(pairs.as_slice()),
            Err(ContextSourceInfoError::MalformedMediaType { key: MediaTypeKey::Accept, .. })
        ));
    }

    #[test]
    fn a_conformance_value_that_is_not_major_dot_minor_is_rejected() {
        for value in ["v1", "1", "1.5.2", "one.five"] {
            let pairs = [pair("ngsildConformance", value)];
            assert!(
                matches!(
                    ContextSourceInfo::try_from(pairs.as_slice()),
                    Err(ContextSourceInfoError::InvalidConformance(_))
                ),
                "{value} must be rejected"
            );
        }
    }

    #[test]
    fn a_jsonld_context_that_is_not_an_absolute_url_is_rejected() {
        for value in ["http://exa mple.org/ctx", "/ngsi-ld/v1/jsonldContexts/ctx", "context.jsonld"] {
            let pairs = [pair("jsonldContext", value)];
            assert!(
                matches!(
                    ContextSourceInfo::try_from(pairs.as_slice()),
                    Err(ContextSourceInfoError::InvalidJsonLdContext(_))
                ),
                "{value} must be rejected"
            );
        }
    }
}
