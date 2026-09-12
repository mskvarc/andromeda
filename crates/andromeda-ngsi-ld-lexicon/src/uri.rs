//! The identifier type every NGSI-LD resource is named by (ETSI GS CIM 009 clause 4.6.2).

use derive_more::{Display, From};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// The Namespace Identifier for NGSI-LD URNs.
pub const NGSI_LD_URN_NID: &str = "ngsi-ld";

/// NGSI-LD entity identifier (ETSI GS CIM 009 clause 4.6.2).
///
/// An identifier is a URN (e.g. `urn:ngsi-ld:Building:001`) or a URL
/// (e.g. `https://example.com/entity/1`). Clause 4.6.2 NOTE 2 makes the term
/// "URI" also cover an Internationalized Resource Identifier (IETF RFC 3987),
/// so an identifier whose characters run beyond ASCII -- `urn:ngsi-ld:Ciudad:PARIS`
/// with a non-ASCII vowel, a Relationship object naming such an entity -- is
/// equally admissible and must survive a round trip character for character.
/// [`Url`] cannot carry one: its cannot-be-a-base encoder rewrites every
/// non-ASCII octet as a percent escape, so those identifiers are held verbatim
/// as an [`IriBuf`] instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, From, Display)]
#[serde(try_from = "String", into = "String")]
pub enum Uri {
    /// A URN identifier (e.g., `urn:ngsi-ld:Building:001`).
    #[display("{_0}")]
    Urn(Urn),
    /// A URL identifier (e.g., `https://example.com/entity/1`).
    #[display("{_0}")]
    Url(Url),
    /// An identifier that is neither a URN nor an all-ASCII URL: an IRI carried
    /// verbatim so its non-ASCII characters are neither lost nor escaped
    /// (clause 4.6.2 NOTE 2, IETF RFC 3987).
    #[display("{_0}")]
    Iri(IriBuf),
}

/// Error returned when a string cannot be parsed as a [`Uri`].
#[derive(Debug, Error)]
#[error("invalid URI: not a valid URN, URL or IRI")]
pub struct UriParseError;

impl Uri {
    /// The identifier as a string slice, in whichever form it was parsed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Uri::Urn(urn) => urn.as_str(),
            Uri::Url(url) => url.as_str(),
            Uri::Iri(iri) => iri.as_str(),
        }
    }
}

// `#[as_ref(str)]` applies to a single field; this is a sum of foreign types
// that each spell the accessor differently.
impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

// Validating parser: clause 4.6.2 admits a URN or a URL, and NOTE 2 extends
// both to IRIs. A URN is tried first so its NID case-folding (Annex A.3)
// applies. An all-ASCII remainder is a URL, whose parser normalises scheme and
// authority; a remainder carrying non-ASCII is kept verbatim as an IRI, because
// the URL parser would percent-escape it and break the round trip.
impl FromStr for Uri {
    type Err = UriParseError;

    fn from_str(s: &str) -> Result<Uri, UriParseError> {
        if let Ok(urn) = s.parse::<Urn>() {
            return Ok(Uri::Urn(urn));
        }
        if s.is_ascii() {
            return s.parse::<Url>().map(Uri::Url).map_err(|_| UriParseError);
        }
        IriBuf::new(s.to_string()).map(Uri::Iri).map_err(|_| UriParseError)
    }
}

// Parsing lives in `FromStr`; this adapter exists only because
// `#[serde(try_from = "String")]` requires an owned-string conversion.
impl TryFrom<String> for Uri {
    type Error = UriParseError;

    fn try_from(raw: String) -> Result<Uri, UriParseError> {
        raw.parse()
    }
}

// The wire form is the identifier's own spelling, produced by `Display`; this
// exists only because `#[serde(into = "String")]` demands an `Into<String>`.
impl From<Uri> for String {
    fn from(uri: Uri) -> String {
        uri.to_string()
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

    /// Annex A.3: "it is assumed that context-broker implementations shall
    /// always use lowercase letters in namespaces where they have a choice in
    /// case [...] Restricting the namespace prefix to lower case urn:ngsi-ld:
    /// can improve caching and retrieval."
    ///
    /// URN equivalence is case-insensitive in the scheme and NID, so two
    /// spellings of one identifier have to converge on the lowercase prefix
    /// before anything compares or caches them. The namespace-specific string
    /// is case-sensitive and must survive untouched, or `Building:1` and
    /// `building:1` would collapse into one entity.
    #[test]
    fn a_urn_prefix_is_normalised_to_lowercase_while_the_identifier_keeps_its_case() {
        let parsed: Uri = "URN:NGSI-LD:Building:001".parse().expect("a URN in any case");
        pretty_assertions::assert_eq!(parsed.to_string(), "urn:ngsi-ld:Building:001");

        let mixed: Uri = "uRn:NgSi-Ld:Building:001".parse().expect("a URN in any case");
        pretty_assertions::assert_eq!(mixed, parsed, "the prefix case does not distinguish two identifiers");

        let other_case: Uri = "urn:ngsi-ld:building:001".parse().expect("a lowercase identifier");
        assert_ne!(other_case, parsed, "the namespace specific string stays case-sensitive");
    }

    /// Clause 4.6.2 NOTE 2 makes an identifier carrying non-ASCII characters a
    /// valid NGSI-LD URI as an IRI (IETF RFC 3987). It parses and, crucially,
    /// serialises back to the identical spelling: the URL parser would rewrite
    /// the non-ASCII octets as `%C3%A9`, changing the identifier the client
    /// gave.
    #[test]
    fn a_non_ascii_urn_round_trips_verbatim_rather_than_percent_encoded() {
        let raw = "urn:ngsi-ld:Ciudad:Par\u{00ed}s";
        let parsed: Uri = raw.parse().expect("a non-ASCII URN is a valid IRI");
        assert!(matches!(parsed, Uri::Iri(_)), "a non-ASCII identifier is held as an IRI: {parsed:?}");
        pretty_assertions::assert_eq!(parsed.as_str(), raw);

        let json = serde_json::to_string(&parsed).expect("serialises");
        pretty_assertions::assert_eq!(json, format!("{raw:?}"), "no percent-encoding on the way out");
        let reparsed: Uri = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(reparsed, parsed);
    }

    /// A non-ASCII http(s) identifier is an IRI just as a non-ASCII URN is, and
    /// is likewise kept verbatim rather than handed to the URL parser.
    #[test]
    fn a_non_ascii_http_identifier_is_kept_verbatim_as_an_iri() {
        let raw = "https://example.com/Ciudad/Par\u{00ed}s";
        let parsed: Uri = raw.parse().expect("a non-ASCII URL is a valid IRI");
        assert!(matches!(parsed, Uri::Iri(_)), "{parsed:?}");
        pretty_assertions::assert_eq!(parsed.as_str(), raw);
    }
}
