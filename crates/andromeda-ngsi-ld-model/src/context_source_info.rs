//! The well-known `contextSourceInfo` keys (ETSI GS CIM 009 clause 4.3.6.6).
//!
//! A registration's `contextSourceInfo` is a free-form [`KeyValuePair`] array
//! that is forwarded as headers when the registered source is contacted
//! (clause 6.3.19). Four keys are reserved: clause 4.3.6.6 gives them a fixed
//! meaning and a restriction on their value, and clause 5.9.2.4 makes a
//! violation of those restrictions a `BadRequestData` error at registration
//! time. This module is the one reading of the array both the registration
//! policy (validation) and forwarding (application) share.

use crate::key_value_pair::KeyValuePair;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use iri_rs::IriRefBuf;
use mediatype::{
    MediaType,
    MediaTypeBuf,
    Name,
    names::{APPLICATION, JSON},
};
use std::str::FromStr;
use thiserror::Error;

/// The `accept` key: the media type the source must answer in.
pub const ACCEPT_KEY: &str = "accept";
/// The `contentType` key: the media type a forwarded payload is sent as.
pub const CONTENT_TYPE_KEY: &str = "contentType";
/// The `jsonldContext` key: the context a forwarded request is compacted against.
pub const JSONLD_CONTEXT_KEY: &str = "jsonldContext";
/// The `ngsildConformance` key: the specification version the source understands.
pub const NGSILD_CONFORMANCE_KEY: &str = "ngsildConformance";

/// Why a `contextSourceInfo` array violates clause 4.3.6.6.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextSourceInfoError {
    /// `accept` or `contentType` is not one of the two media types the clause allows.
    #[error("contextSourceInfo {key} must be application/json or application/ld+json, got {value}")]
    UnsupportedMediaType {
        /// Which of the two media-type keys carried the value.
        key: &'static str,
        /// The offending value.
        value: String,
    },
    /// `jsonldContext` is not a URL reference.
    #[error("contextSourceInfo jsonldContext must be a URL reference, got {0}")]
    InvalidJsonLdContext(String),
    /// `ngsildConformance` is not of the form `major.minor`.
    #[error("contextSourceInfo ngsildConformance must be of the form major.minor, got {0}")]
    InvalidConformance(String),
}

/// The reserved keys of a `contextSourceInfo` array, read into their types, and
/// the remaining pairs that are forwarded verbatim (clause 6.3.19).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSourceInfo<'a> {
    /// The media type the registered source shall answer in.
    pub accept: Option<MediaTypeBuf>,
    /// The media type a forwarded payload shall be sent as.
    pub content_type: Option<MediaTypeBuf>,
    /// The JSON-LD context a forwarded request shall be compacted against.
    pub jsonld_context: Option<IriRefBuf>,
    /// The specification version the forwarded payload shall conform to.
    pub ngsild_conformance: Option<SpecVersion>,
    /// Every pair outside the reserved keys, in registration order.
    pub custom: Vec<&'a KeyValuePair>,
}

/// Whether a media type is one clause 4.3.6.6 admits for `accept` and `contentType`.
fn is_admitted_media_type(media_type: &MediaType<'_>) -> bool {
    media_type.ty == APPLICATION
        && (media_type.subty == JSON && media_type.suffix.is_none() || media_type.subty == Name::new_unchecked("ld") && media_type.suffix == Some(JSON))
}

/// Parse one of the two media-type keys under the clause 4.3.6.6 restriction.
fn parse_media_type(key: &'static str, value: &str) -> Result<MediaTypeBuf, ContextSourceInfoError> {
    let unsupported = || ContextSourceInfoError::UnsupportedMediaType { key, value: value.to_string() };
    let parsed = MediaType::parse(value).map_err(|_| unsupported())?;
    if !is_admitted_media_type(&parsed) {
        return Err(unsupported());
    }
    Ok(MediaTypeBuf::from(parsed))
}

impl<'a> TryFrom<&'a [KeyValuePair]> for ContextSourceInfo<'a> {
    type Error = ContextSourceInfoError;

    fn try_from(pairs: &'a [KeyValuePair]) -> Result<ContextSourceInfo<'a>, ContextSourceInfoError> {
        let mut info = ContextSourceInfo::default();
        for pair in pairs {
            match pair.key.as_str() {
                ACCEPT_KEY => info.accept = Some(parse_media_type(ACCEPT_KEY, &pair.value)?),
                CONTENT_TYPE_KEY => info.content_type = Some(parse_media_type(CONTENT_TYPE_KEY, &pair.value)?),
                JSONLD_CONTEXT_KEY => {
                    info.jsonld_context =
                        Some(IriRefBuf::new(pair.value.to_string()).map_err(|_| ContextSourceInfoError::InvalidJsonLdContext(pair.value.to_string()))?);
                }
                NGSILD_CONFORMANCE_KEY => {
                    info.ngsild_conformance =
                        Some(SpecVersion::from_str(&pair.value).map_err(|_| ContextSourceInfoError::InvalidConformance(pair.value.to_string()))?);
                }
                _custom => info.custom.push(pair),
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source_info::{ContextSourceInfo, ContextSourceInfoError},
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
        pretty_assertions::assert_eq!(info.custom, vec![&pairs[4]]);
    }

    #[test]
    fn a_media_type_outside_the_two_admitted_ones_is_rejected() {
        for (key, value) in [("accept", "text/plain"), ("contentType", "application/xml"), ("accept", "not a media type")] {
            let pairs = [pair(key, value)];
            assert!(
                matches!(
                    ContextSourceInfo::try_from(pairs.as_slice()),
                    Err(ContextSourceInfoError::UnsupportedMediaType { .. })
                ),
                "{key}: {value} must be rejected"
            );
        }
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
    fn a_jsonld_context_that_is_not_a_url_reference_is_rejected() {
        let pairs = [pair("jsonldContext", "http://exa mple.org/ctx")];
        assert!(matches!(
            ContextSourceInfo::try_from(pairs.as_slice()),
            Err(ContextSourceInfoError::InvalidJsonLdContext(_))
        ));
    }
}
