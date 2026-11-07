//! The identifier that addresses one stored `@context` (clause 5.13.4.3).
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.13.4.3 gives the input of Serve @context as
//! "the locally unique identifier that identifies the desired @context in the
//! broker's internal storage. Such unique identifiers are obtained by the
//! client as a result of either a 'Add @context' (clause 5.13.2) API operation
//! or of a 'List @contexts' (clause 5.13.3) API operation. For @contexts of
//! kind 'Cached' this can also be the original URL the broker downloaded the
//! @context from." Clause 5.13.5.3 says the same of Delete and Reload.
//!
//! Both spellings are URIs, and every stored row is keyed by one. Something
//! else -- whatever a client puts in the path segment -- therefore addresses no
//! entry, and clauses 5.13.4.4 and 5.13.5.4 both answer that with
//! `ResourceNotFound`: "It shall give back ResourceNotFound if the identifier
//! is not found", and "If the @context identifier does not correspond to any
//! existing entry in the @context storage, then an error of type
//! ResourceNotFound shall be raised". Only a *missing* identifier is
//! `BadRequestData`, and a path segment is never missing.
//!
//! This type is what keeps that distinction: it holds the segment as written,
//! and the stored key it resolves to when it resolves to one at all.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use derive_more::Display;

/// A `{contextId}` path segment, and the storage key it addresses.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
#[display("{written}")]
pub struct ContextIdentifier {
    /// The segment exactly as the request wrote it.
    written: String,
    /// The storage key it addresses, when it is one at all.
    stored: Option<Identifier>,
}

impl ContextIdentifier {
    /// The storage key this identifier addresses, if it addresses one.
    ///
    /// `None` is not a malformed request: it is an identifier no stored row can
    /// be keyed by, which the clause answers with `ResourceNotFound`.
    #[must_use]
    pub const fn stored(&self) -> Option<&Identifier> {
        self.stored.as_ref()
    }
}

// Every identifier is admissible, so this is a total conversion rather than a
// validating parse: what a segment cannot do is *address* a row.
impl From<&str> for ContextIdentifier {
    fn from(written: &str) -> ContextIdentifier {
        ContextIdentifier {
            written: written.to_owned(),
            stored: written.parse().ok(),
        }
    }
}

// A key the broker already holds addresses itself.
impl From<Identifier> for ContextIdentifier {
    fn from(stored: Identifier) -> ContextIdentifier {
        ContextIdentifier {
            written: stored.to_string(),
            stored: Some(stored),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::identifier::ContextIdentifier;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    /// Clause 5.13.4.3's two spellings: the local identifier a creation handed
    /// back, and the original URL of a cached `@context`.
    #[test]
    fn both_spellings_the_clause_admits_address_a_stored_row() {
        for written in [
            "urn:ngsi-ld:JsonLdContext:2fc7f63e-9213-4821-8720-65b9a404df6f",
            "https://example.com/terms.jsonld",
        ] {
            let identifier = ContextIdentifier::from(written);
            pretty_assertions::assert_eq!(identifier.stored().map(ToString::to_string), Some(written.to_owned()));
        }
    }

    /// A segment that is no URI keys no row, which is a lookup that finds
    /// nothing rather than a request the broker refuses.
    #[test]
    fn a_segment_that_is_no_uri_addresses_nothing() {
        for written in ["wrong_id_context", "1234567890123456", ""] {
            let identifier = ContextIdentifier::from(written);
            assert!(identifier.stored().is_none(), "{written} must address nothing");
            pretty_assertions::assert_eq!(identifier.to_string(), written);
        }
    }

    /// A key the broker already holds addresses itself.
    #[test]
    fn a_stored_key_addresses_itself() {
        let key: Identifier = "urn:ngsi-ld:JsonLdContext:1".parse().expect("a valid URI");
        let identifier = ContextIdentifier::from(key.clone());
        pretty_assertions::assert_eq!(identifier.stored(), Some(&key));
    }
}
