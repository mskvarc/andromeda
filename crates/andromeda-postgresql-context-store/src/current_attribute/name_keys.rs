//! The dictionary keys flattening writes, and the names assembly reads them
//! back as.
//!
//! Rows and elements hold every name as a key: an attribute name as an
//! `attribute_names` key, and every IRI in a JSON-LD `@type` position (a
//! literal's datatype, a `valueType`, an `objectType`) as an `entity_type_names`
//! key. Both maps hold hits only; a name with no key cannot be flattened, and a
//! key with no name cannot be assembled.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReference};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
use std::collections::HashMap;

/// The keys of the names an Entity's attributes mention.
#[derive(Debug, Default)]
pub struct NameKeys {
    /// `attribute_names` keys.
    attributes: ResolvedNames<AttributeName>,
    /// `entity_type_names` keys.
    types: ResolvedNames<EntityTypeName>,
}

impl NameKeys {
    /// The keys of the given attribute names and type IRIs.
    #[must_use]
    pub const fn new(attributes: ResolvedNames<AttributeName>, types: ResolvedNames<EntityTypeName>) -> NameKeys {
        NameKeys { attributes, types }
    }

    /// The `attribute_names` key of `name`.
    #[must_use]
    pub fn attribute(&self, name: &AttributeName) -> Option<AttributeNameKey> {
        self.attributes.get(name)
    }

    /// The `entity_type_names` key of the type IRI `name`.
    #[must_use]
    pub fn type_name(&self, name: &NgsiLdReference) -> Option<EntityTypeNameKey> {
        self.types.get_str(name.as_str())
    }
}

/// The names behind the keys an Entity's rows hold.
#[derive(Debug, Default)]
pub struct KeyNames {
    /// Attribute names by `attribute_names` key.
    attributes: HashMap<AttributeNameKey, AttributeName>,
    /// Type IRIs by `entity_type_names` key.
    types: HashMap<EntityTypeNameKey, NgsiLdReference>,
}

impl KeyNames {
    /// The names behind the given keys.
    pub fn from_pairs(
        attributes: impl IntoIterator<Item = (AttributeNameKey, AttributeName)>,
        types: impl IntoIterator<Item = (EntityTypeNameKey, NgsiLdReference)>,
    ) -> KeyNames {
        KeyNames {
            attributes: attributes.into_iter().collect(),
            types: types.into_iter().collect(),
        }
    }

    /// The attribute name behind `key`.
    #[must_use]
    pub fn attribute(&self, key: AttributeNameKey) -> Option<&AttributeName> {
        self.attributes.get(&key)
    }

    /// The type IRI behind `key`.
    #[must_use]
    pub fn type_name(&self, key: EntityTypeNameKey) -> Option<&NgsiLdReference> {
        self.types.get(&key)
    }
}
