//! The name of an entity type (ETSI GS CIM 009 clause 5.2.4, `type` member).

use crate::reference::{NgsiLdReferenceError, validate_ngsi_ld_name};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, Into};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

validated_ngsi_name! {
    /// Name of an entity type (clause 5.2.4).
    ///
    /// Compact payloads name types by term or compact IRI; expanded payloads
    /// name them by full IRI. Both are accepted, because both reach this type
    /// depending on which side of JSON-LD expansion the payload sits on. The
    /// grammar is the one every named NGSI-LD member shares — see
    /// [`validate_ngsi_ld_name`].
    EntityTypeName,
    derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, AsRef, Display, Deref, Into),
    as_ref(str),
    borrow_str
}

#[cfg(test)]
mod tests {
    use crate::{entity_type_name::EntityTypeName, identifier_length::IDENTIFIER_BYTE_LIMIT, reference::NgsiLdReferenceError};
    use iri_rs::IriBuf;

    #[test]
    fn an_entity_type_name_accepts_terms_prefixes_and_expanded_iris() {
        for raw in ["Building", "schema:Place", "https://example.com/Vehicle"] {
            pretty_assertions::assert_eq!(raw.parse::<EntityTypeName>().expect(raw).as_str(), raw);
        }
    }

    #[test]
    fn an_entity_type_name_rejects_empty_and_whitespace_bearing_input() {
        assert!("".parse::<EntityTypeName>().is_err());
        assert!("two words".parse::<EntityTypeName>().is_err());
    }

    name_length_tests!(EntityTypeName);
}
