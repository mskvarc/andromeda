//! A dereferenceable URI that survives a round trip unchanged.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.9-1 restricts a Context Source
//! Registration's `endpoint` to "a dereferenceable URI", and Table 5.2.15-1
//! says the same of a Subscription's notification endpoint `uri`. A
//! [`url::Url`] is what decides that: it is the type the broker later builds a
//! forwarded request from, so a value it cannot parse is not dereferenceable.
//!
//! What a `Url` is not is a faithful record of what the writer sent. Parsing
//! normalizes — `http://host:1026` gains the empty path it implies and comes
//! back as `http://host:1026/`, a scheme and host are lower-cased, a default
//! port is dropped. Clause 5.2.2 makes the members a Context Producer supplies
//! ones the broker stores and serves back, and none of the three tables that
//! define these members admits a canonical form in place of the written one, so
//! a client that registers `http://my.csource.org:1026` has to read
//! `http://my.csource.org:1026` back.
//!
//! This type therefore keeps both: the text exactly as written, which is what
//! serializes, and the parsed URL, which is what the broker dereferences.

use derive_more::{AsRef, Display};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
use std::str::FromStr;
use thiserror::Error;
use url::Url;

/// A URI that parses as a dereferenceable URL and renders as it was written.
///
/// Equality is the written spelling, and needs no field to be excluded from it:
/// the spelling is what the parse consumed, so it decides the URL rather than
/// standing beside it. Two values that render differently are two different
/// things to serve back, which is the distinction this type exists to keep; a
/// caller asking instead where a request would go compares [`Self::url`].
#[derive(AsRef, Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("{written}")]
pub struct DereferenceableUri {
    /// The URI exactly as its writer spelled it.
    #[as_ref(str)]
    written: String,
    /// The parsed form every request against this URI is built from.
    dereferenced: Url,
}

/// A URI that cannot be dereferenced.
#[derive(Debug, Error)]
#[error("value is not a dereferenceable URI: {source}")]
pub struct NotDereferenceable {
    /// Why the URL parser refused it.
    source: url::ParseError,
}

impl DereferenceableUri {
    /// The URL a request against this URI is built from.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.dereferenced
    }

    /// The URI as its writer spelled it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.written
    }
}

// Validating parser: the URL decides whether the value is dereferenceable at
// all, and the input decides how it renders. No derive pairs the two.
impl FromStr for DereferenceableUri {
    type Err = NotDereferenceable;

    fn from_str(value: &str) -> Result<DereferenceableUri, NotDereferenceable> {
        match Url::parse(value) {
            Ok(dereferenced) => Ok(DereferenceableUri {
                written: value.to_owned(),
                dereferenced,
            }),
            Err(source) => Err(NotDereferenceable { source }),
        }
    }
}

// A URL that was already parsed has no written spelling of its own, so it
// renders canonically. `From` rather than `TryFrom` because every `Url` is by
// construction dereferenceable.
impl From<Url> for DereferenceableUri {
    fn from(dereferenced: Url) -> DereferenceableUri {
        DereferenceableUri {
            written: dereferenced.as_str().to_owned(),
            dereferenced,
        }
    }
}

// Serialization is the written spelling, which is the whole point of the type;
// deserialization validates through the URL parser.
impl Serialize for DereferenceableUri {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.written)
    }
}

impl<'de> Deserialize<'de> for DereferenceableUri {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<DereferenceableUri, D::Error> {
        let written = String::deserialize(deserializer)?;
        written.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::dereferenceable_uri::DereferenceableUri;
    use url::Url;

    /// A registration written without a trailing slash reads back without one,
    /// where a bare `Url` would add the empty path it implies.
    #[test]
    fn an_authority_only_uri_keeps_the_spelling_it_was_written_with() {
        let endpoint: DereferenceableUri = "http://my.csource.org:1026".parse().expect("a dereferenceable URI");
        assert_eq!(endpoint.as_str(), "http://my.csource.org:1026");
        assert_eq!(endpoint.url().as_str(), "http://my.csource.org:1026/");
    }

    /// The trailing slash a client did write is kept too: the type records the
    /// spelling rather than choosing one.
    #[test]
    fn a_trailing_slash_that_was_written_is_kept() {
        let endpoint: DereferenceableUri = "http://my.csource.org:1026/".parse().expect("a dereferenceable URI");
        assert_eq!(endpoint.as_str(), "http://my.csource.org:1026/");
    }

    /// A path, a query and a fragment all survive verbatim.
    #[test]
    fn a_path_survives_verbatim() {
        for written in [
            "http://my.csource.org:1026/broker1",
            "https://my.csource.org/ngsi-ld/v1",
            "http://[2001:db8::1]:1026/csource",
        ] {
            let endpoint: DereferenceableUri = written.parse().expect("a dereferenceable URI");
            assert_eq!(endpoint.as_str(), written);
        }
    }

    /// Table 5.2.9-1 restricts the member to a dereferenceable URI, so a
    /// relative reference is refused rather than resolved against something.
    #[test]
    fn a_relative_reference_is_not_dereferenceable() {
        let error = "/broker1".parse::<DereferenceableUri>().expect_err("a relative reference is refused");
        pretty_assertions::assert_eq!(error.source, url::ParseError::RelativeUrlWithoutBase);
    }

    /// Equality is the written spelling, and the parsed URL is what says where
    /// two spellings would send the same request.
    #[test]
    fn equality_is_the_spelling_and_the_url_is_the_destination() {
        let written: DereferenceableUri = "http://my.csource.org:1026".parse().expect("a dereferenceable URI");
        let slashed: DereferenceableUri = "http://my.csource.org:1026/".parse().expect("a dereferenceable URI");
        assert_ne!(written, slashed, "the two spellings are two things to serve back");
        assert_eq!(written.url(), slashed.url(), "and one place to send a request");
        let elsewhere: DereferenceableUri = "http://other.csource.org:1026/".parse().expect("a dereferenceable URI");
        assert_ne!(written.url(), elsewhere.url());
    }

    /// A URL that reached the broker already parsed has no written spelling of
    /// its own, so it renders canonically.
    #[test]
    fn a_parsed_url_renders_canonically() {
        let url = Url::parse("http://my.csource.org:1026").expect("a valid URL");
        let endpoint = DereferenceableUri::from(url);
        assert_eq!(endpoint.as_str(), "http://my.csource.org:1026/");
    }

    /// The wire form is the written spelling on the way out and is validated on
    /// the way in.
    #[test]
    fn the_wire_form_round_trips_the_written_spelling() {
        let endpoint: DereferenceableUri = "http://my.csource.org:1026".parse().expect("a dereferenceable URI");
        let encoded = serde_json::to_value(&endpoint).expect("serialises");
        pretty_assertions::assert_eq!(encoded, serde_json::json!("http://my.csource.org:1026"));
        let decoded: DereferenceableUri = serde_json::from_value(encoded).expect("deserialises");
        assert_eq!(decoded.as_str(), "http://my.csource.org:1026");
        assert!(serde_json::from_value::<DereferenceableUri>(serde_json::json!("/broker1")).is_err());
    }
}
