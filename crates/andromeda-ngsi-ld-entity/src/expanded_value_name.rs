//! A name inside a structured value as JSON-LD expansion keeps it: the name of
//! a member of a node, of a foreign member of a `GeoJSON` geometry, or of a
//! node's `@type` entry.
//!
//! A Property value is any JSON value (clause 3.1, Table 5.2.5-1), and clause
//! 4.6.3 obliges a broker to support every JSON native type, so the keys of an
//! object inside a value are any strings RFC 8259 admits. The name grammar of
//! clause 4.6.2 restricts Entity Type, Property and Relationship names only,
//! and clause 4.6.4 lets a broker refuse a value for its listed injection
//! characters only. Expansion turns each key into what IRI expansion against
//! the active context makes of it (`opening hours` becomes
//! `https://uri.etsi.org/ngsi-ld/default-context/opening hours`) and keeps
//! every result holding a `:` whether or not it is a well-formed IRI (JSON-LD
//! 1.1 expansion algorithm step 13.3); compaction turns it back into the key
//! the client wrote. An `@type` entry is expanded the same way, vocabulary
//! relative, so `Postal Address` keeps its space too.
//!
//! Two texts are no such name. One holding no `:` is a key expansion drops,
//! and one starting with `@` is a keyword's place in an expanded document,
//! where it could not be read back as a name.

use compact_str::CompactString;
use derive_more::{AsRef, Display};
use std::{borrow::Borrow, str::FromStr};
use thiserror::Error;

/// Why a text is no expanded name inside a structured value.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ExpandedValueNameError {
    /// The text holds no `:`; JSON-LD expansion drops such a key.
    #[error("a name inside a structured value holds no ':', so JSON-LD expansion does not keep it")]
    NoColon,
    /// The text starts with `@`, which an expanded document reserves for
    /// keywords.
    #[error("a name inside a structured value starts with '@', which an expanded document reserves for keywords")]
    KeywordSpace,
}

/// An expanded name inside a structured value.
#[derive(AsRef, Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct ExpandedValueName(CompactString);

impl ExpandedValueName {
    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: no derive expresses a fallible parse.
impl FromStr for ExpandedValueName {
    type Err = ExpandedValueNameError;

    fn from_str(text: &str) -> Result<ExpandedValueName, ExpandedValueNameError> {
        if text.starts_with('@') {
            return Err(ExpandedValueNameError::KeywordSpace);
        }
        if !text.contains(':') {
            return Err(ExpandedValueNameError::NoColon);
        }
        Ok(ExpandedValueName(CompactString::from(text)))
    }
}

// No derive produces `Borrow` for a target other than the field's own type.
// Sound because the derived `Hash` forwards to `CompactString`, which hashes
// exactly as its `str` does.
impl Borrow<str> for ExpandedValueName {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::expanded_value_name::{ExpandedValueName, ExpandedValueNameError};
    use indexmap::IndexMap;

    /// Clause 4.6.2 restricts attribute names only: a key inside a value keeps
    /// whatever text expansion made of it.
    #[test]
    fn a_name_that_is_no_well_formed_iri_is_kept() {
        for text in [
            "https://uri.etsi.org/ngsi-ld/default-context/opening hours",
            "https://example.org/a",
            "_:b0",
            "urn:x:1",
        ] {
            let name: ExpandedValueName = text.parse().expect("a kept name");
            pretty_assertions::assert_eq!(name.as_str(), text);
        }
    }

    #[test]
    fn a_name_expansion_drops_or_a_keyword_is_refused() {
        pretty_assertions::assert_eq!("opening hours".parse::<ExpandedValueName>(), Err(ExpandedValueNameError::NoColon));
        pretty_assertions::assert_eq!("".parse::<ExpandedValueName>(), Err(ExpandedValueNameError::NoColon));
        pretty_assertions::assert_eq!("@value".parse::<ExpandedValueName>(), Err(ExpandedValueNameError::KeywordSpace));
        pretty_assertions::assert_eq!("@x:y".parse::<ExpandedValueName>(), Err(ExpandedValueNameError::KeywordSpace));
    }

    #[test]
    fn a_map_keyed_by_names_is_probed_with_text() {
        let name: ExpandedValueName = "https://example.org/a b".parse().expect("a kept name");
        let map = IndexMap::from([(name, 1_u8)]);
        pretty_assertions::assert_eq!(map.get("https://example.org/a b"), Some(&1));
    }
}
