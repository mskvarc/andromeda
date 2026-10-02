//! The member names of an expanded JSON-LD object, read once.
//!
//! Every object of an expanded document is keyed by JSON-LD keywords and by
//! expanded IRIs. The key is classified as it is read, so the visitors match on
//! a typed key rather than comparing strings.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use serde::{
    Deserialize,
    Deserializer,
    de::{Error, Visitor},
};
use std::fmt;

/// One member name of an expanded object.
#[derive(Debug)]
pub(crate) enum ObjectKey {
    /// `@id`.
    Id,
    /// `@type`.
    Type,
    /// `@value`.
    Value,
    /// `@language`.
    Language,
    /// `@direction`.
    Direction,
    /// `@list`.
    List,
    /// Any other keyword, which the entity model admits nowhere.
    OtherKeyword,
    /// An expanded IRI.
    Name(AttributeName),
    /// A member name that is no NGSI-LD name.
    InvalidName(NgsiLdReferenceError),
}

impl ObjectKey {
    /// Classify one member name.
    fn classify(text: &str) -> ObjectKey {
        match text {
            "@id" => ObjectKey::Id,
            "@type" => ObjectKey::Type,
            "@value" => ObjectKey::Value,
            "@language" => ObjectKey::Language,
            "@direction" => ObjectKey::Direction,
            "@list" => ObjectKey::List,
            keyword if keyword.starts_with('@') => ObjectKey::OtherKeyword,
            name => name.parse::<AttributeName>().map_or_else(ObjectKey::InvalidName, ObjectKey::Name),
        }
    }
}

impl<'de> Deserialize<'de> for ObjectKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObjectKey, D::Error> {
        deserializer.deserialize_str(ObjectKeyVisitor)
    }
}

/// Reads one member name.
struct ObjectKeyVisitor;

impl Visitor<'_> for ObjectKeyVisitor {
    type Value = ObjectKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: Error>(self, text: &str) -> Result<ObjectKey, E> {
        Ok(ObjectKey::classify(text))
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::object_key::ObjectKey;

    #[test]
    fn keywords_and_names_are_told_apart() {
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""@value""#), Ok(ObjectKey::Value)));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""@index""#), Ok(ObjectKey::OtherKeyword)));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""https://ex.org/a""#), Ok(ObjectKey::Name(_))));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""""#), Ok(ObjectKey::InvalidName(_))));
    }
}
