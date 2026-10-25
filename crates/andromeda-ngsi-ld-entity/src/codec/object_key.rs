//! The member names of an expanded JSON-LD object, read once.
//!
//! Every object of an expanded document is keyed by JSON-LD keywords and by
//! expanded names. The key is classified as it is read, so the visitors match
//! on a typed key rather than comparing strings. A name is read as the wider
//! [`ExpandedValueName`] a member of a structured value may carry; an entity or
//! an attribute instance, whose members are attributes and NGSI-LD members,
//! narrows it to an [`AttributeName`] with [`attribute_key`].

use crate::{
    expanded_value_name::{ExpandedValueName, ExpandedValueNameError},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
    /// An expanded name.
    Name(ExpandedValueName),
    /// A member name expansion never produces.
    InvalidName(ExpandedValueNameError),
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
            name => name.parse::<ExpandedValueName>().map_or_else(ObjectKey::InvalidName, ObjectKey::Name),
        }
    }
}

/// The attribute name the member `name` of an entity or an attribute instance
/// is keyed by.
///
/// # Errors
///
/// Returns [`EntityRefusal::ExpandedAttributeName`] for a name that is no
/// attribute name.
pub(crate) fn attribute_key(name: &ExpandedValueName) -> Result<AttributeName, EntityRefusal> {
    name.as_str().parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)
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
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        codec::object_key::{ObjectKey, attribute_key},
        refusal::EntityRefusal,
    };

    #[test]
    fn keywords_and_names_are_told_apart() {
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""@value""#), Ok(ObjectKey::Value)));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""@index""#), Ok(ObjectKey::OtherKeyword)));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""https://ex.org/a""#), Ok(ObjectKey::Name(_))));
        assert!(matches!(serde_json::from_str::<ObjectKey>(r#""""#), Ok(ObjectKey::InvalidName(_))));
    }

    /// A member of a structured value keeps the name expansion made of its key;
    /// an attribute is held to the name grammar.
    #[test]
    fn a_name_with_a_space_is_a_value_member_but_no_attribute() {
        let Ok(ObjectKey::Name(name)) = serde_json::from_str::<ObjectKey>(r#""https://ex.org/opening hours""#) else {
            panic!("a value member name");
        };
        assert!(matches!(attribute_key(&name), Err(EntityRefusal::ExpandedAttributeName(_))));
    }
}
