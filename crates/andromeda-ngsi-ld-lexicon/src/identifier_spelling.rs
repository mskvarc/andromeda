//! An identifier held only as the spelling it is stored and compared in, for a
//! structure that keeps one for each of many resources.

use crate::identifier::{Identifier, IdentifierParseError};
use arcstr::ArcStr;
use derive_more::Display;
use std::borrow::Borrow;

/// An [`Identifier`] held only as the spelling it is stored and compared in, in
/// one allocation every clone shares.
///
/// An [`Identifier`] keeps its URI parsed as well, 88 bytes beside its text. A
/// structure that keeps an identifier for each of many resources, and only
/// hashes, compares and hands them on, holds this instead: a one-word handle
/// and one allocation of a reference count, a length and the text.
///
/// Two spellings are equal when their text is, which is how the store tells
/// two resources apart. Identifier equality is coarser for a URN: IETF RFC 8141
/// section 3 leaves the r-, q- and f-components out of URN equivalence, so two
/// identifiers that differ only there are equal as identifiers and distinct as
/// spellings, as they are in the store.
///
/// A spelling is made only from an [`Identifier`], so its text is always one an
/// identifier spells.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("{_0}")]
pub struct IdentifierSpelling(ArcStr);

impl IdentifierSpelling {
    /// The spelling as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&Identifier> for IdentifierSpelling {
    fn from(identifier: &Identifier) -> IdentifierSpelling {
        IdentifierSpelling(ArcStr::from(identifier.as_str()))
    }
}

// Parsing the spelling an identifier produced gives that identifier back. The
// conversion is fallible all the same, because that rests on each parser
// accepting its own output unchanged, which no type enforces.
impl TryFrom<&IdentifierSpelling> for Identifier {
    type Error = IdentifierParseError;

    fn try_from(spelling: &IdentifierSpelling) -> Result<Identifier, IdentifierParseError> {
        spelling.as_str().parse()
    }
}

// No derive produces `Borrow`. Sound because the derived `Hash` and `Eq` forward
// to the text, so a map keyed by spellings can be probed with an identifier's
// own spelling without allocating a second one.
impl Borrow<str> for IdentifierSpelling {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::{identifier::Identifier, identifier_spelling::IdentifierSpelling};
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, ptr};

    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("a well-formed identifier")
    }

    #[test]
    fn a_spelling_parses_back_into_the_identifier_it_was_made_from() {
        for raw in [
            "urn:ngsi-ld:Vehicle:A",
            "URN:NGSI-LD:Vehicle:A",
            "HTTPS://Example.com/vehicles/1",
            "urn:ngsi-ld:Ciudad:Par\u{ed}s",
            "https://example.com/Ciudad/Par\u{ed}s",
        ] {
            let original = identifier(raw);
            let spelling = IdentifierSpelling::from(&original);
            assert_eq!(spelling.as_str(), original.as_str(), "{raw}");
            assert_eq!(Identifier::try_from(&spelling), Ok(original), "{raw}");
        }
    }

    #[test]
    fn every_clone_of_a_spelling_shares_its_text() {
        let spelling = IdentifierSpelling::from(&identifier("urn:ngsi-ld:Vehicle:A"));
        let clone = spelling.clone();
        assert!(ptr::eq(spelling.as_str(), clone.as_str()));
    }

    /// IETF RFC 8141 section 3 leaves the f-component out of URN equivalence,
    /// while the store compares the text.
    #[test]
    fn urns_differing_only_in_an_f_component_are_one_identifier_and_two_spellings() {
        let plain = identifier("urn:ngsi-ld:Vehicle:A");
        let fragment = identifier("urn:ngsi-ld:Vehicle:A#front");
        assert_eq!(plain, fragment);
        assert_ne!(IdentifierSpelling::from(&plain), IdentifierSpelling::from(&fragment));
    }

    #[test]
    fn a_set_of_spellings_is_probed_with_an_identifiers_own_spelling() {
        let held = identifier("https://example.com/vehicles/1");
        let spellings = HashSet::from([IdentifierSpelling::from(&held)]);
        assert!(spellings.contains(held.as_str()));
        assert!(!spellings.contains(identifier("https://example.com/vehicles/2").as_str()));
    }
}
