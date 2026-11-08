//! The URI every NGSI-LD resource is named and referenced by (ETSI GS CIM 009 clause 4.6.2).

use derive_more::Display;
use iri_rs::{Iri, IriBuf};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// The Namespace Identifier for NGSI-LD URNs.
pub const NGSI_LD_URN_NID: &str = "ngsi-ld";

/// A URI as ETSI GS CIM 009 clause 4.6.2 admits it.
///
/// A URI is a URN (e.g. `urn:ngsi-ld:Building:001`) or a URL
/// (e.g. `https://example.com/entity/1`). Clause 4.6.2 NOTE 2 makes the term
/// "URI" also cover an Internationalized Resource Identifier (IETF RFC 3987),
/// so an identifier whose characters run beyond ASCII (`urn:ngsi-ld:Ciudad:PARIS`
/// with a non-ASCII vowel, or a Relationship object naming such an entity) is
/// equally admissible and must survive a round trip character for character.
/// [`Url`] cannot carry one: its cannot-be-a-base encoder rewrites every
/// non-ASCII octet as a percent escape, so those identifiers are held verbatim
/// as an [`IriBuf`] instead.
///
/// Neither clause 4.6.2 nor IETF RFC 3986 and RFC 3987 bound a URI's length,
/// and neither does this type: a `Uri` is whatever the grammar admits, and only
/// the request-body limit bounds how long one can be. The URI a stored resource
/// is named by is an [`Identifier`], which holds a `Uri` to the bound its index
/// needs. The representation is private so that no construction path can
/// bypass the grammar; [`Uri::view`] borrows the form it was parsed as.
///
/// [`Identifier`]: crate::identifier::Identifier
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Display)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
pub struct Uri(UriForm);

/// The three forms an identifier is held in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
enum UriForm {
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

/// A borrowed view of the form a [`Uri`] was parsed as.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum UriView<'a> {
    /// A URN identifier (e.g., `urn:ngsi-ld:Building:001`).
    #[display("{_0}")]
    Urn(&'a Urn),
    /// A URL identifier (e.g., `https://example.com/entity/1`).
    #[display("{_0}")]
    Url(&'a Url),
    /// A non-ASCII identifier held verbatim as an IRI (clause 4.6.2 NOTE 2).
    #[display("{_0}")]
    Iri(&'a IriBuf),
}

/// The string is not a URN, a URL or an IRI.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("invalid URI: not a valid URN, URL or IRI")]
pub struct UriParseError;

impl Uri {
    /// The identifier as a string slice, in whichever form it was parsed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            UriForm::Urn(urn) => urn.as_str(),
            UriForm::Url(url) => url.as_str(),
            UriForm::Iri(iri) => iri.as_str(),
        }
    }

    /// Borrow the form the identifier was parsed as.
    #[must_use]
    pub const fn view(&self) -> UriView<'_> {
        match &self.0 {
            UriForm::Urn(urn) => UriView::Urn(urn),
            UriForm::Url(url) => UriView::Url(url),
            UriForm::Iri(iri) => UriView::Iri(iri),
        }
    }

    /// The identifier as a URN, when it was parsed as one.
    #[must_use]
    pub const fn as_urn(&self) -> Option<&Urn> {
        match &self.0 {
            UriForm::Urn(urn) => Some(urn),
            UriForm::Url(_) | UriForm::Iri(_) => None,
        }
    }

    /// The identifier as a URL, when it was parsed as one.
    #[must_use]
    pub const fn as_url(&self) -> Option<&Url> {
        match &self.0 {
            UriForm::Url(url) => Some(url),
            UriForm::Urn(_) | UriForm::Iri(_) => None,
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

/// Hold `raw` to the IETF RFC 3986 grammar, or the IETF RFC 3987 one when it
/// carries non-ASCII (clause 4.6.2 NOTE 2).
///
/// The WHATWG URL parser behind [`Url`] is not a URI validator: it keeps a
/// literal space in the opaque path of a scheme such as `urn`, strips tabs and
/// line breaks anywhere, and trims leading and trailing spaces, so
/// `urn:ngsi-ld:Building:has space` would otherwise be an identifier. No form
/// is parsed before the grammar admits the whole string.
fn well_formed(raw: &str) -> Result<(), UriParseError> {
    Iri::parse(raw).map(drop).map_err(|_| UriParseError)
}

// Validating parser: clause 4.6.2 admits a URN or a URL, and NOTE 2 extends
// both to IRIs. Once the grammar admits the string, a URN is tried first so its
// NID case-folding (Annex A.3) applies. An all-ASCII remainder is a URL, whose
// parser normalises scheme and authority; a remainder carrying non-ASCII is kept
// verbatim as an IRI, because the URL parser would percent-escape it and break
// the round trip.
impl FromStr for Uri {
    type Err = UriParseError;

    fn from_str(s: &str) -> Result<Uri, UriParseError> {
        well_formed(s)?;
        let form = if let Ok(urn) = s.parse::<Urn>() {
            UriForm::Urn(urn)
        } else if s.is_ascii() {
            s.parse::<Url>().map(UriForm::Url).map_err(|_| UriParseError)?
        } else {
            IriBuf::new(s.to_string()).map(UriForm::Iri).map_err(|_| UriParseError)?
        };
        Ok(Uri(form))
    }
}

// A URN is always a URI.
impl From<Urn> for Uri {
    fn from(urn: Urn) -> Uri {
        Uri(UriForm::Urn(urn))
    }
}

// A WHATWG URL is not always a URI: an opaque path such as the one of
// `urn:a b` keeps its literal space. Its serialisation is parsed like any other
// spelling, so a `urn:` URL is held as the URN it is: the derived equality
// compares the form first, and one string held in two forms would name two
// resources.
impl TryFrom<Url> for Uri {
    type Error = UriParseError;

    fn try_from(url: Url) -> Result<Uri, UriParseError> {
        url.as_str().parse()
    }
}

// An IRI is always a URI (clause 4.6.2 NOTE 2). It is held in the form its
// spelling parses into, for the same reason as a URL; only a spelling the
// parser refuses, which no other construction can produce, stays an IRI.
impl From<IriBuf> for Uri {
    fn from(iri: IriBuf) -> Uri {
        match iri.as_str().parse() {
            Ok(uri) => uri,
            Err(UriParseError) => Uri(UriForm::Iri(iri)),
        }
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
    use crate::uri::{Uri, UriParseError, UriView};
    use iri_rs::IriBuf;
    use std::hash::{DefaultHasher, Hash, Hasher};
    use url::Url;
    use urn_rs::Urn;

    /// A string of exactly `length` octets that starts with `prefix`.
    fn of_length(prefix: &str, length: usize) -> String {
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    #[test]
    fn uri_urn_json_round_trip() {
        let uri: Uri = "urn:ngsi-ld:Building:001".parse().unwrap();
        let json = serde_json::to_string(&uri).unwrap();
        pretty_assertions::assert_eq!(json, "\"urn:ngsi-ld:Building:001\"");
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_url_json_round_trip() {
        let uri: Uri = "https://example.com/entity/1".parse().unwrap();
        assert!(matches!(uri.view(), UriView::Url(_)));
        let json = serde_json::to_string(&uri).unwrap();
        let parsed: Uri = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed, uri);
    }

    #[test]
    fn uri_display_format() {
        let urn_uri: Uri = "urn:ngsi-ld:Thing:1".parse().unwrap();
        pretty_assertions::assert_eq!(urn_uri.to_string(), "urn:ngsi-ld:Thing:1");
    }

    #[test]
    fn uri_from_urn() {
        let urn: Urn = "urn:ngsi-ld:Building:001".parse().unwrap();
        let uri = Uri::from(urn);
        assert!(matches!(uri.view(), UriView::Urn(_)));
        assert!(uri.as_urn().is_some());
    }

    #[test]
    fn uri_from_url() {
        let url: Url = "https://example.com/entity/1".parse().unwrap();
        let uri = Uri::try_from(url).unwrap();
        assert!(matches!(uri.view(), UriView::Url(_)));
        assert!(uri.as_url().is_some());
    }

    fn hash_of(uri: &Uri) -> u64 {
        let mut hasher = DefaultHasher::new();
        uri.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn a_urn_spelling_is_one_uri_whether_it_arrives_as_a_string_a_url_or_an_iri() {
        let raw = "urn:ngsi-ld:FedTrip:001";
        let parsed: Uri = raw.parse().expect("a URN");
        let from_url = Uri::try_from(Url::parse(raw).expect("a URL")).expect("a URI");
        let from_iri = Uri::from(IriBuf::new(raw.to_owned()).expect("an IRI"));
        for other in [&from_url, &from_iri] {
            assert!(matches!(other.view(), UriView::Urn(_)), "{other:?}");
            pretty_assertions::assert_eq!(other, &parsed);
            pretty_assertions::assert_eq!(hash_of(other), hash_of(&parsed));
        }
    }

    #[test]
    fn an_ascii_iri_is_held_as_the_url_its_spelling_parses_into() {
        let raw = "https://example.com/entity/1";
        let parsed: Uri = raw.parse().expect("a URL");
        let from_iri = Uri::from(IriBuf::new(raw.to_owned()).expect("an IRI"));
        assert!(matches!(from_iri.view(), UriView::Url(_)), "{from_iri:?}");
        pretty_assertions::assert_eq!(from_iri, parsed);
        pretty_assertions::assert_eq!(hash_of(&from_iri), hash_of(&parsed));
    }

    #[test]
    fn a_urn_prefix_is_normalised_to_lowercase_while_the_identifier_keeps_its_case() {
        let parsed: Uri = "URN:NGSI-LD:Building:001".parse().expect("a URN in any case");
        pretty_assertions::assert_eq!(parsed.to_string(), "urn:ngsi-ld:Building:001");

        let mixed: Uri = "uRn:NgSi-Ld:Building:001".parse().expect("a URN in any case");
        pretty_assertions::assert_eq!(mixed, parsed, "the prefix case does not distinguish two identifiers");

        let other_case: Uri = "urn:ngsi-ld:building:001".parse().expect("a lowercase identifier");
        assert_ne!(other_case, parsed, "the namespace specific string stays case-sensitive");
    }

    #[test]
    fn a_non_ascii_urn_round_trips_verbatim_rather_than_percent_encoded() {
        let raw = "urn:ngsi-ld:Ciudad:Par\u{00ed}s";
        let parsed: Uri = raw.parse().expect("a non-ASCII URN is a valid IRI");
        assert!(matches!(parsed.view(), UriView::Iri(_)), "a non-ASCII identifier is held as an IRI: {parsed:?}");
        pretty_assertions::assert_eq!(parsed.as_str(), raw);

        let json = serde_json::to_string(&parsed).expect("serialises");
        pretty_assertions::assert_eq!(json, format!("{raw:?}"), "no percent-encoding on the way out");
        let reparsed: Uri = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(reparsed, parsed);
    }

    #[test]
    fn a_non_ascii_http_identifier_is_kept_verbatim_as_an_iri() {
        let raw = "https://example.com/Ciudad/Par\u{00ed}s";
        let parsed: Uri = raw.parse().expect("a non-ASCII URL is a valid IRI");
        assert!(matches!(parsed.view(), UriView::Iri(_)), "{parsed:?}");
        pretty_assertions::assert_eq!(parsed.as_str(), raw);
    }

    #[test]
    fn whitespace_in_a_urn_a_url_or_an_iri_is_malformed() {
        for raw in [
            "urn:ngsi-ld:Building:has space",
            "urn:ngsi-ld:Building:has\ttab",
            "urn:ngsi-ld:Building:has\nnewline",
            "urn:ngsi-ld:Building:trailing ",
            " urn:ngsi-ld:Building:leading",
            "https://example.com/has space",
            "https://example.com/has\ttab",
            "https://example.com/has\nnewline",
            "https://example.com/?q=has space",
            "https://example.com/Par\u{ed}s has space",
            "https://example.com/Par\u{ed}s\ttab",
            "https://example.com/Par\u{ed}s\nnewline",
            "urn:ngsi-ld:Ciudad:Par\u{ed}s has space",
        ] {
            pretty_assertions::assert_eq!(raw.parse::<Uri>(), Err(UriParseError), "{raw:?}");
        }
    }

    #[test]
    fn characters_outside_the_uri_grammar_are_malformed() {
        for raw in [
            "urn:ngsi-ld:Building:a<b",
            "urn:ngsi-ld:Building:a\"b",
            "urn:ngsi-ld:Building:a{b}",
            "urn:ngsi-ld:Building:a|b",
            "urn:ngsi-ld:Building:a\\b",
            "urn:ngsi-ld:Building:a^b",
            "urn:ngsi-ld:Building:a`b",
            "urn:ngsi-ld:Building:a%zz",
            "https://example.com/a%2",
        ] {
            pretty_assertions::assert_eq!(raw.parse::<Uri>(), Err(UriParseError), "{raw:?}");
        }
    }

    #[test]
    fn a_percent_encoded_space_is_well_formed() {
        for raw in ["urn:ngsi-ld:Building:has%20space", "https://example.com/has%20space?q=a%20b#f%20g"] {
            let parsed: Uri = raw.parse().expect("a percent-encoded space is a URI character");
            pretty_assertions::assert_eq!(parsed.as_str(), raw);
        }
    }

    #[test]
    fn a_foreign_url_with_a_literal_space_cannot_become_a_uri() {
        let url: Url = "urn:ngsi-ld:Building:has space".parse().expect("the WHATWG parser keeps the space");
        pretty_assertions::assert_eq!(Uri::try_from(url), Err(UriParseError));
    }

    /// Clause 4.6.2 sets no length on a URI, and neither do IETF RFC 3986 and
    /// RFC 3987: a URN, a URL and an IRI of 6000 octets are URIs, and each
    /// survives the round trip byte for byte.
    #[test]
    fn a_uri_of_any_length_is_a_uri() {
        for raw in [
            of_length("urn:ngsi-ld:Building:", 6000),
            of_length("https://example.com/entity/", 6000),
            format!("https://example.com/Ciudad/Par\u{ed}s/{}", "\u{e9}".repeat(3000)),
        ] {
            let parsed: Uri = raw.parse().expect("a URI of any length");
            pretty_assertions::assert_eq!(parsed.as_str(), raw);
            let json = serde_json::to_string(&parsed).expect("serialises");
            pretty_assertions::assert_eq!(serde_json::from_str::<Uri>(&json).expect("deserialises"), parsed);
        }
    }

    #[test]
    fn a_urn_of_one_mebibyte_is_a_uri() {
        let raw = of_length("urn:ngsi-ld:Building:", 1 << 20);
        let parsed: Uri = raw.parse().expect("a URN of any length");
        pretty_assertions::assert_eq!(parsed.as_str().len(), 1 << 20);
    }

    #[test]
    fn a_long_non_uri_is_still_malformed() {
        let raw = format!("not a uri {}", "x".repeat(6000));
        pretty_assertions::assert_eq!(raw.parse::<Uri>(), Err(UriParseError));
    }

    #[test]
    fn a_foreign_urn_url_or_iri_of_any_length_becomes_a_uri() {
        let urn: Urn = of_length("urn:ngsi-ld:Building:", 6000).parse().expect("a URN");
        pretty_assertions::assert_eq!(Uri::from(urn).as_str().len(), 6000);

        let url: Url = of_length("https://example.com/", 6000).parse().expect("a URL");
        pretty_assertions::assert_eq!(Uri::try_from(url).map(|uri| uri.as_str().len()), Ok(6000));

        let iri = IriBuf::new(format!("https://example.com/{}", "\u{e9}".repeat(3000))).expect("an IRI");
        pretty_assertions::assert_eq!(Uri::from(iri).as_str().len(), 6020);
    }
}
