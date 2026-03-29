use derive_more::From;
use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;
use urn::Urn;

/// The Namespace Identifier for NGSI-LD URNs.
pub const NGSI_LD_URN_NID: &str = "ngsi-ld";

/// An NGSI-LD entity identifier — either a URN or a URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, From)]
#[serde(untagged)]
pub enum Uri {
    /// A URN identifier (e.g., `urn:ngsi-ld:Building:001`).
    Urn(Urn),
    /// A URL identifier (e.g., `https://example.com/entity/1`).
    Url(Url),
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Uri::Urn(urn) => write!(f, "{}", urn),
            Uri::Url(url) => write!(f, "{}", url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_urn_json_round_trip() {
        let uri = Uri::Urn("urn:ngsi-ld:Building:001".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        assert_eq!(json, "\"urn:ngsi-ld:Building:001\"");
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_url_json_round_trip() {
        let uri = Uri::Url("https://example.com/entity/1".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_display_format() {
        let urn_uri = Uri::Urn("urn:ngsi-ld:Thing:1".parse().unwrap());
        assert_eq!(urn_uri.to_string(), "urn:ngsi-ld:Thing:1");
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
