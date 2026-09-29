//! The identifier a stored NGSI-LD resource is named by (ETSI GS CIM 009 clause 4.6.2).

use crate::{
    identifier_length::IdentifierTooLong,
    uri::{Uri, UriParseError},
};
use derive_more::{AsRef, Deref, Display, Into};
use iri_rs::{Iri, IriBuf};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};
use thiserror::Error;
use url::Url;
use urn_rs::Urn;

/// The URI one stored resource is identified by: an Entity, a Subscription, a
/// Context Source Registration, a Snapshot, an `EntityMap`, a JSON-LD
/// `@context`, and the `datasetId` and `instanceId` of an attribute instance.
///
/// Clause 4.6.2 types every one of these as a URI and sets no length on it.
/// Each is nonetheless a key in a B-tree index, and PostgreSQL refuses a B-tree
/// entry larger than 2704 bytes, so an identifier is a [`Uri`] held to
/// [`IDENTIFIER_BYTE_LIMIT`] octets in the spelling it is stored and compared
/// in. That bound is Andromeda's own implementation limit, refused as
/// `BadRequestData` before anything reaches the store.
///
/// A reference to a resource -- a Relationship's `object`, a member of a
/// `ListRelationship`'s `objectList` -- is a plain [`Uri`] instead: it is never an
/// index key, so nothing but the request-body limit bounds it. A reference that
/// is longer than an identifier may be cannot name a stored resource, which is
/// what [`Identifier::try_from`] a [`Uri`] answers.
///
/// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
#[derive(AsRef, Clone, Debug, Deref, Deserialize, Display, Eq, Hash, Into, PartialEq, Serialize)]
#[as_ref(forward)]
#[into(String, Uri)]
#[serde(try_from = "String", into = "Uri")]
#[display("{_0}")]
pub struct Identifier(Uri);

/// Why a string is not an [`Identifier`].
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum IdentifierParseError {
    /// The string is not a URI at all.
    #[error(transparent)]
    Malformed(#[from] UriParseError),
    /// The string is a URI, but longer than an identifier may be.
    #[error(transparent)]
    TooLong(#[from] IdentifierTooLong),
}

impl Identifier {
    /// The identifier as a string slice, in the spelling it is stored in.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Borrow the identifier as the URI it is.
    #[must_use]
    pub const fn as_uri(&self) -> &Uri {
        &self.0
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
    pub fn from_static(iri: Iri<&'static str>) -> Identifier {
        Identifier(Uri::from(IriBuf::from(iri)))
    }

    /// Wrap a URN whose spelling a crate-local parser has already held to the
    /// identifier bound.
    pub(crate) fn from_bounded_urn(urn: Urn) -> Identifier {
        Identifier(Uri::from(urn))
    }
}

// No derive produces `Borrow` for the field's own type through a newtype.
// Sound because the derived `Hash` and `Eq` forward to the wrapped `Uri`, so a
// map keyed by identifiers can be probed with a reference, which is how a
// Relationship's object finds the Entity it names.
impl Borrow<Uri> for Identifier {
    fn borrow(&self) -> &Uri {
        &self.0
    }
}

// The bound is what distinguishes an identifier from any other URI, so the
// conversion is fallible on length alone.
impl TryFrom<Uri> for Identifier {
    type Error = IdentifierTooLong;

    fn try_from(uri: Uri) -> Result<Identifier, IdentifierTooLong> {
        IdentifierTooLong::check(uri.as_str())?;
        Ok(Identifier(uri))
    }
}

// A URN is always a URI; only its length can refuse it.
impl TryFrom<Urn> for Identifier {
    type Error = IdentifierTooLong;

    fn try_from(urn: Urn) -> Result<Identifier, IdentifierTooLong> {
        Identifier::try_from(Uri::from(urn))
    }
}

// A WHATWG URL is held to the URI grammar first, then to the bound.
impl TryFrom<Url> for Identifier {
    type Error = IdentifierParseError;

    fn try_from(url: Url) -> Result<Identifier, IdentifierParseError> {
        Ok(Identifier::try_from(Uri::try_from(url)?)?)
    }
}

// The bound applies to the normalised spelling `Uri` parses into, because that
// is the one stored: the URL parser lengthens a string by percent-encoding an
// apostrophe in the query of a special scheme.
impl FromStr for Identifier {
    type Err = IdentifierParseError;

    fn from_str(raw: &str) -> Result<Identifier, IdentifierParseError> {
        Ok(Identifier::try_from(raw.parse::<Uri>()?)?)
    }
}

// Parsing lives in `FromStr`; this adapter exists only because
// `#[serde(try_from = "String")]` requires an owned-string conversion.
impl TryFrom<String> for Identifier {
    type Error = IdentifierParseError;

    fn try_from(raw: String) -> Result<Identifier, IdentifierParseError> {
        raw.parse()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        identifier::{Identifier, IdentifierParseError},
        identifier_length::{IDENTIFIER_BYTE_LIMIT, IdentifierTooLong},
        uri::{Uri, UriParseError},
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use url::Url;
    use urn_rs::Urn;

    /// A URN whose spelling is exactly `length` octets long.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Building:";
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    #[test]
    fn a_urn_of_exactly_the_limit_is_an_identifier() {
        let parsed: Identifier = urn_of_length(IDENTIFIER_BYTE_LIMIT).parse().expect("a URN at the limit");
        assert_eq!(parsed.as_str().len(), IDENTIFIER_BYTE_LIMIT);
    }

    #[test]
    fn a_urn_one_byte_past_the_limit_is_too_long() {
        assert_eq!(
            urn_of_length(IDENTIFIER_BYTE_LIMIT + 1).parse::<Identifier>(),
            Err(IdentifierParseError::TooLong(IdentifierTooLong {
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
        assert_eq!(
            raw.parse::<Identifier>(),
            Err(IdentifierParseError::TooLong(IdentifierTooLong { length: raw.len() }))
        );
    }

    /// A non-ASCII IRI (clause 4.6.2 NOTE 2) whose two-octet characters bring
    /// it to exactly the limit is an identifier, and one more octet is not.
    #[test]
    fn a_non_ascii_iri_at_the_limit_in_octets_is_an_identifier() {
        let prefix = "urn:ngsi-ld:Ciudad:";
        let at_limit = format!("{prefix}x{}", "\u{e9}".repeat((IDENTIFIER_BYTE_LIMIT - prefix.len() - 1) / 2));
        assert_eq!(at_limit.len(), IDENTIFIER_BYTE_LIMIT);
        assert_eq!(
            at_limit.parse::<Identifier>().map(|identifier| identifier.as_str().len()),
            Ok(IDENTIFIER_BYTE_LIMIT)
        );
        assert!(format!("{at_limit}a").parse::<Identifier>().is_err());
    }

    /// The URL parser percent-encodes an apostrophe in the query of a special
    /// scheme, although IETF RFC 3986 admits it there, so a 1023-octet URL with
    /// one apostrophe is stored as 1025 octets and is refused on that stored
    /// length.
    #[test]
    fn the_limit_applies_to_the_normalised_spelling() {
        let prefix = "https://example.com/?a'b";
        let raw = format!("{prefix}{}", "c".repeat(IDENTIFIER_BYTE_LIMIT - 1 - prefix.len()));
        assert_eq!(raw.len(), IDENTIFIER_BYTE_LIMIT - 1);
        assert_eq!(
            raw.parse::<Identifier>(),
            Err(IdentifierParseError::TooLong(IdentifierTooLong {
                length: IDENTIFIER_BYTE_LIMIT + 1
            }))
        );
    }

    #[test]
    fn a_non_uri_is_malformed_rather_than_too_long() {
        let raw = format!("not a uri {}", "x".repeat(IDENTIFIER_BYTE_LIMIT));
        assert_eq!(raw.parse::<Identifier>(), Err(IdentifierParseError::Malformed(UriParseError)));
    }

    #[test]
    fn deserialising_an_over_long_identifier_fails_with_its_length() {
        let json = serde_json::Value::String(urn_of_length(IDENTIFIER_BYTE_LIMIT + 1));
        let error = serde_json::from_value::<Identifier>(json).expect_err("an over-long identifier");
        assert!(error.to_string().contains("1025 bytes"), "{error}");
    }

    #[test]
    fn an_identifier_round_trips_through_json_as_its_uri() {
        let identifier: Identifier = "urn:ngsi-ld:Building:001".parse().expect("a valid identifier");
        let json = serde_json::to_string(&identifier).expect("serialises");
        assert_eq!(json, "\"urn:ngsi-ld:Building:001\"");
        assert_eq!(serde_json::from_str::<Identifier>(&json).expect("deserialises"), identifier);
    }

    /// A reference is any URI; it is an identifier only when it is short enough
    /// to name a stored resource.
    #[test]
    fn a_uri_becomes_an_identifier_only_within_the_limit() {
        let short: Uri = urn_of_length(IDENTIFIER_BYTE_LIMIT).parse().expect("a URI");
        assert_eq!(Identifier::try_from(short.clone()).map(Uri::from), Ok(short)); // clone: compared after the move

        let long: Uri = urn_of_length(6000).parse().expect("a URI of any length");
        assert_eq!(Identifier::try_from(long), Err(IdentifierTooLong { length: 6000 }));
    }

    #[test]
    fn a_foreign_urn_url_or_iri_past_the_limit_is_no_identifier() {
        let urn: Urn = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1).parse().expect("a URN");
        assert!(Identifier::try_from(urn).is_err());

        let url: Url = format!("https://example.com/{}", "a".repeat(IDENTIFIER_BYTE_LIMIT)).parse().expect("a URL");
        let uri = Uri::try_from(url).expect("a URL is a URI of any length");
        assert!(Identifier::try_from(uri).is_err());

        let iri = IriBuf::new(format!("https://example.com/{}", "\u{e9}".repeat(IDENTIFIER_BYTE_LIMIT))).expect("an IRI");
        assert!(Identifier::try_from(Uri::from(iri)).is_err());
    }

    #[test]
    fn an_identifier_reads_as_the_uri_it_wraps() {
        let identifier: Identifier = "https://example.com/entity/1".parse().expect("a valid identifier");
        assert!(identifier.as_url().is_some(), "an identifier exposes the URI's own views");
        assert_eq!(AsRef::<str>::as_ref(&identifier), "https://example.com/entity/1");
        assert_eq!(identifier.as_uri().as_str(), "https://example.com/entity/1");
    }
}
