//! The name types a tenant dictionary interns.

use crate::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use std::{borrow::Borrow, fmt::Debug, hash::Hash, str::FromStr};

/// A validated NGSI-LD name that a per-tenant dictionary maps onto an
/// `INTEGER` key.
///
/// The two implementors are the expanded attribute IRI and the expanded
/// entity type IRI. Both share the clause 4.6.2 name grammar and both are
/// stored once in a dictionary and referenced by key everywhere else; what
/// differs is the key type, which is what keeps an attribute key from ever
/// being bound where a type key belongs.
pub trait InternedName: Clone + Eq + Hash + Borrow<str> + FromStr + Debug + Send + Sync + 'static {
    /// The dictionary key this name type resolves to.
    type Key: Copy + Eq + Hash + Debug + From<i32> + Send + Sync + 'static;

    /// The name's text, as the dictionary stores it.
    fn as_str(&self) -> &str;

    /// The `INTEGER` a key binds as.
    fn key_value(key: Self::Key) -> i32;
}

impl InternedName for AttributeName {
    type Key = AttributeNameKey;

    fn as_str(&self) -> &str {
        AttributeName::as_str(self)
    }

    fn key_value(key: AttributeNameKey) -> i32 {
        key.get()
    }
}

impl InternedName for EntityTypeName {
    type Key = EntityTypeNameKey;

    fn as_str(&self) -> &str {
        EntityTypeName::as_str(self)
    }

    fn key_value(key: EntityTypeNameKey) -> i32 {
        key.get()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{attribute_name_key::AttributeNameKey, dictionary::interned_name::InternedName, entity_type_name_key::EntityTypeNameKey};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

    #[test]
    fn an_attribute_name_is_interned_under_the_attribute_key() {
        let name: AttributeName = "https://example.com/speed".parse().expect("valid name");
        pretty_assertions::assert_eq!(InternedName::as_str(&name), "https://example.com/speed");
        pretty_assertions::assert_eq!(<AttributeName as InternedName>::key_value(AttributeNameKey::from(3)), 3);
    }

    #[test]
    fn an_entity_type_name_is_interned_under_the_type_key() {
        let name: EntityTypeName = "https://example.com/Vehicle".parse().expect("valid name");
        pretty_assertions::assert_eq!(InternedName::as_str(&name), "https://example.com/Vehicle");
        pretty_assertions::assert_eq!(<EntityTypeName as InternedName>::key_value(EntityTypeNameKey::from(9)), 9);
    }
}
