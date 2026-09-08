//! The identifier type every NGSI-LD resource is named by (ETSI GS CIM 009 clause 4.6.2).

use derive_more::{Display, From};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// The Namespace Identifier for NGSI-LD URNs.
pub const NGSI_LD_URN_NID: &str = "ngsi-ld";

/// NGSI-LD entity identifier (ETSI GS CIM 009 clause 4.6.2).
///
/// An entity identifier is either a URN (e.g., `urn:ngsi-ld:Building:001`)
/// or a URL (e.g., `https://example.com/entity/1`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, From, Display)]
#[serde(untagged)]
pub enum Uri {
    /// A URN identifier (e.g., `urn:ngsi-ld:Building:001`).
    #[display("{_0}")]
    Urn(Urn),
    /// A URL identifier (e.g., `https://example.com/entity/1`).
    #[display("{_0}")]
    Url(Url),
}

/// Error returned when a string cannot be parsed as a [`Uri`].
#[derive(Debug, Error)]
#[error("invalid URI: not a valid URN or URL")]
pub struct UriParseError;

impl Uri {
    /// The identifier as a string slice, in whichever form it was parsed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Uri::Urn(urn) => urn.as_str(),
            Uri::Url(url) => url.as_str(),
        }
    }
}

// `#[as_ref(str)]` applies to a single field; this is a sum of two foreign
// types that each spell the accessor differently.
impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

// Validating parser: clause 4.6.2 admits either a URN or a URL, so the parse
// tries them in that order. No derive expresses an ordered fallback.
impl FromStr for Uri {
    type Err = UriParseError;

    fn from_str(s: &str) -> Result<Uri, UriParseError> {
        if let Ok(urn) = s.parse::<Urn>() {
            return Ok(Uri::Urn(urn));
        }
        s.parse::<Url>().map(Uri::Url).map_err(|_| UriParseError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_urn_json_round_trip() {
        let uri = Uri::Urn("urn:ngsi-ld:Building:001".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        pretty_assertions::assert_eq!(json, "\"urn:ngsi-ld:Building:001\"");
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_url_json_round_trip() {
        let uri = Uri::Url("https://example.com/entity/1".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_display_format() {
        let urn_uri = Uri::Urn("urn:ngsi-ld:Thing:1".parse().unwrap());
        pretty_assertions::assert_eq!(urn_uri.to_string(), "urn:ngsi-ld:Thing:1");
    }

    #[test]
    fn uri_from_urn() {
        let urn: Urn = "urn:ngsi-ld:Building:001".parse().unwrap();
        let uri: Uri = urn.into();
        assert!(matches!(uri, Uri::Urn(_)));
    }

    #[test]
    fn uri_from_url() {
        let url: Url = "https://example.com/entity/1".parse().unwrap();
        let uri: Uri = url.into();
        assert!(matches!(uri, Uri::Url(_)));
    }
}
