//! The identifier type every NGSI-LD resource is named by (ETSI GS CIM 009 clause 4.6.2).

use crate::identifier_length::IdentifierTooLong;
use derive_more::Display;
use iri_rs::{Iri, IriBuf};
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
///
/// Every value is at most [`IDENTIFIER_BYTE_LIMIT`] octets long in the spelling
/// it is stored and compared in. The representation is private so that no
/// construction path can bypass that bound; [`Uri::view`] borrows the form it
/// was parsed as.
///
/// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
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

/// Error returned when a string cannot be parsed as a [`Uri`].
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum UriParseError {
    /// The string is not a URN, a URL or an IRI.
    #[error("invalid URI: not a valid URN, URL or IRI")]
    Malformed,
    /// The string is a URI, but longer than an identifier may be.
    #[error(transparent)]
    TooLong(#[from] IdentifierTooLong),
}

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

    /// An identifier from a fixed IRI literal the caller wrote.
    ///
    /// The literal's grammar is checked at compile time by `iri!`; its length
    /// is not, so the caller passing a literal it wrote is the guarantor that it
    /// is within [`IDENTIFIER_BYTE_LIMIT`]. This exists for built-in
    /// configuration defaults, where the literal is compile-time fixed and
    /// covered by a test, so no fallible construction belongs on the path.
    ///
    /// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
    #[must_use]
    pub fn from_static(iri: Iri<&'static str>) -> Uri {
        Uri(UriForm::Iri(IriBuf::from(iri)))
    }

    /// Wrap a URN whose spelling a crate-local parser has already held to the
    /// identifier bound.
    pub(crate) const fn from_bounded_urn(urn: Urn) -> Uri {
        Uri(UriForm::Urn(urn))
    }

    /// Admit `form` once its spelling is within the identifier bound.
    fn bounded(form: UriForm) -> Result<Uri, IdentifierTooLong> {
        let uri = Uri(form);
        IdentifierTooLong::check(uri.as_str())?;
        Ok(uri)
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
// the URL parser would percent-escape it and break the round trip. The length
// bound applies to the normalised spelling, because that is the one stored:
// the URL parser lengthens a string by percent-encoding a space.
impl FromStr for Uri {
    type Err = UriParseError;

    fn from_str(s: &str) -> Result<Uri, UriParseError> {
        let form = if let Ok(urn) = s.parse::<Urn>() {
            UriForm::Urn(urn)
        } else if s.is_ascii() {
            s.parse::<Url>().map(UriForm::Url).map_err(|_| UriParseError::Malformed)?
        } else {
            IriBuf::new(s.to_string()).map(UriForm::Iri).map_err(|_| UriParseError::Malformed)?
        };
        Ok(Uri::bounded(form)?)
    }
}

// A URN is always a valid identifier form; only its length can refuse it.
impl TryFrom<Urn> for Uri {
    type Error = IdentifierTooLong;

    fn try_from(urn: Urn) -> Result<Uri, IdentifierTooLong> {
        Uri::bounded(UriForm::Urn(urn))
    }
}

// A URL is always a valid identifier form; only its length can refuse it.
impl TryFrom<Url> for Uri {
    type Error = IdentifierTooLong;

    fn try_from(url: Url) -> Result<Uri, IdentifierTooLong> {
        Uri::bounded(UriForm::Url(url))
    }
}

// An IRI is always a valid identifier form (clause 4.6.2 NOTE 2); only its
// length can refuse it.
impl TryFrom<IriBuf> for Uri {
    type Error = IdentifierTooLong;

    fn try_from(iri: IriBuf) -> Result<Uri, IdentifierTooLong> {
        Uri::bounded(UriForm::Iri(iri))
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
    use crate::{
        identifier_length::{IDENTIFIER_BYTE_LIMIT, IdentifierTooLong},
        uri::{Uri, UriParseError, UriView},
    };
    use iri_rs::IriBuf;
    use url::Url;
    use urn_rs::Urn;

    /// A URN whose spelling is exactly `length` octets long.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Building:";
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
        let uri = Uri::try_from(urn).unwrap();
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
        assert!(matches!(parsed.view(), UriView::Iri(_)), "a non-ASCII identifier is held as an IRI: {parsed:?}");
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
        assert!(matches!(parsed.view(), UriView::Iri(_)), "{parsed:?}");
        pretty_assertions::assert_eq!(parsed.as_str(), raw);
    }

    #[test]
    fn a_urn_of_exactly_the_limit_parses() {
        let raw = urn_of_length(IDENTIFIER_BYTE_LIMIT);
        let parsed: Uri = raw.parse().expect("a URN at the limit");
        pretty_assertions::assert_eq!(parsed.as_str().len(), IDENTIFIER_BYTE_LIMIT);
    }

    #[test]
    fn a_urn_one_byte_past_the_limit_is_too_long() {
        let raw = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1);
        pretty_assertions::assert_eq!(
            raw.parse::<Uri>(),
            Err(UriParseError::TooLong(IdentifierTooLong {
                length: IDENTIFIER_BYTE_LIMIT + 1
            }))
        );
    }

    /// A two-octet character counts twice: 600 of them are 1200 octets, past
    /// the limit, although the identifier is well under 1024 characters.
    #[test]
    fn the_limit_counts_utf8_octets_not_characters() {
        let raw = format!("urn:ngsi-ld:Building:{}", "\u{e9}".repeat(600));
        assert!(raw.chars().count() < IDENTIFIER_BYTE_LIMIT);
        pretty_assertions::assert_eq!(raw.parse::<Uri>(), Err(UriParseError::TooLong(IdentifierTooLong { length: raw.len() })));
    }

    /// The URL parser percent-encodes a space, so a 1023-octet URL with one
    /// space is stored as 1025 octets and is refused on that stored length.
    #[test]
    fn the_limit_applies_to_the_normalised_spelling() {
        let prefix = "https://example.com/a b/";
        let raw = format!("{prefix}{}", "c".repeat(IDENTIFIER_BYTE_LIMIT - 1 - prefix.len()));
        pretty_assertions::assert_eq!(raw.len(), IDENTIFIER_BYTE_LIMIT - 1);
        pretty_assertions::assert_eq!(
            raw.parse::<Uri>(),
            Err(UriParseError::TooLong(IdentifierTooLong {
                length: IDENTIFIER_BYTE_LIMIT + 1
            }))
        );
    }

    #[test]
    fn a_non_uri_is_malformed_rather_than_too_long() {
        let raw = format!("not a uri {}", "x".repeat(IDENTIFIER_BYTE_LIMIT));
        pretty_assertions::assert_eq!(raw.parse::<Uri>(), Err(UriParseError::Malformed));
    }

    #[test]
    fn deserialising_an_over_long_identifier_fails() {
        let json = serde_json::Value::String(urn_of_length(IDENTIFIER_BYTE_LIMIT + 1));
        let error = serde_json::from_value::<Uri>(json).expect_err("an over-long identifier");
        assert!(error.to_string().contains("1025 bytes"), "{error}");
    }

    #[test]
    fn a_foreign_url_past_the_limit_cannot_become_a_uri() {
        let url: Url = format!("https://example.com/{}", "a".repeat(IDENTIFIER_BYTE_LIMIT)).parse().unwrap();
        let length = url.as_str().len();
        pretty_assertions::assert_eq!(Uri::try_from(url), Err(IdentifierTooLong { length }));

        let urn: Urn = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1).parse().unwrap();
        assert!(Uri::try_from(urn).is_err());

        let iri = IriBuf::new(format!("https://example.com/{}", "\u{e9}".repeat(IDENTIFIER_BYTE_LIMIT))).unwrap();
        assert!(Uri::try_from(iri).is_err());
    }
}
