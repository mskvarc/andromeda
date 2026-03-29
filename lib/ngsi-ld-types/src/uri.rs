use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;
use urn::Urn;

/// The Namespace Identifier for NGSI-LD URNs.
pub const NGSI_LD_URN_NID: &str = "ngsi-ld";

/// An NGSI-LD entity identifier — either a URN or a URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum Uri {
    Urn(Urn),
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

impl From<Urn> for Uri {
    fn from(urn: Urn) -> Self {
        Uri::Urn(urn)
    }
}

impl From<Url> for Uri {
    fn from(url: Url) -> Self {
        Uri::Url(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urn_round_trips_through_json() {
        let uri = Uri::Urn("urn:ngsi-ld:Building:001".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        assert_eq!(json, "\"urn:ngsi-ld:Building:001\"");
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn url_round_trips_through_json() {
        let uri = Uri::Url("https://example.com/entity/1".parse().unwrap());
        let json = serde_json::to_string(&uri).unwrap();
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uri);
    }

    #[test]
    fn display_formats_correctly() {
        let urn_uri = Uri::Urn("urn:ngsi-ld:Thing:1".parse().unwrap());
        assert_eq!(urn_uri.to_string(), "urn:ngsi-ld:Thing:1");
    }
}
