//! The name of an entity type (ETSI GS CIM 009 clause 5.2.4, `type` member).

use crate::reference::{NgsiLdReferenceError, validate_ngsi_ld_name};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, Into};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, str::FromStr};

/// Name of an entity type (clause 5.2.4).
///
/// Compact payloads name types by term or compact IRI; expanded payloads name
/// them by full IRI. Both are accepted, because both reach this type depending
/// on which side of JSON-LD expansion the payload sits on. The grammar is the
/// one every named NGSI-LD member shares — see [`validate_ngsi_ld_name`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, AsRef, Display, Deref, Into)]
#[serde(try_from = "String", into = "String")]
#[as_ref(str)]
#[display("{_0}")]
#[into(String)]
pub struct EntityTypeName(CompactString);

impl EntityTypeName {
    /// Borrow the type name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: the grammar check in `validate_ngsi_ld_name` is what makes
// the newtype worth having, and no derive expresses a fallible parse.
impl FromStr for EntityTypeName {
    type Err = NgsiLdReferenceError;

    fn from_str(value: &str) -> Result<EntityTypeName, NgsiLdReferenceError> {
        validate_ngsi_ld_name(value)?;
        Ok(EntityTypeName(CompactString::from(value)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for EntityTypeName {
    type Error = NgsiLdReferenceError;

    fn try_from(value: String) -> Result<EntityTypeName, NgsiLdReferenceError> {
        value.parse()
    }
}

// No derive produces `Borrow` for a target other than the field's own type,
// and this one lets sets and maps keyed by type name be probed with a `&str`
// without allocating. Sound because the derived `Hash` forwards to
// `CompactString`, which hashes exactly as its `str` does.
impl Borrow<str> for EntityTypeName {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_type_name::EntityTypeName;

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
}
