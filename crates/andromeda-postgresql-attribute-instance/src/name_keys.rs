//! The dictionary keys flattening writes, and the names assembly reads them
//! back as.
//!
//! Rows and elements hold every name as a key: an attribute name as an
//! `attribute_names` key, and every IRI in a JSON-LD `@type` position (a
//! literal's datatype, a `valueType`, an `objectType`) as an `entity_type_names`
//! key. Both maps hold hits only; a name with no key cannot be flattened, and a
//! key with no name cannot be assembled. A write interns the names
//! [`MentionedNames`] collects before it flattens; a read looks up the names of
//! the keys [`HeldKeys`] collects before it assembles.

use crate::{flatten::scalar_literal, stored_members::InstanceMembers, sub_attribute::SubAttributeElement};
use andromeda_ngsi_ld_entity::{attributes::Attributes, family_value::FamilyValue};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReference};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
use std::{collections::HashMap, sync::OnceLock};

/// The keys of the names an Entity's attributes mention.
#[derive(Debug, Default)]
pub struct NameKeys {
    /// `attribute_names` keys.
    attributes: ResolvedNames<AttributeName>,
    /// `entity_type_names` keys.
    types: ResolvedNames<EntityTypeName>,
    /// The type IRI behind each key of `types`, which the equality keys of a
    /// sub-attribute read an element's `valueType` from. Built on the first
    /// such read: most writes hold no element whose `valueType` declares the
    /// members of an array.
    type_names: OnceLock<HashMap<EntityTypeNameKey, NgsiLdReference>>,
}

impl NameKeys {
    /// The keys of the given attribute names and type IRIs.
    #[must_use]
    pub const fn new(attributes: ResolvedNames<AttributeName>, types: ResolvedNames<EntityTypeName>) -> NameKeys {
        NameKeys {
            attributes,
            types,
            type_names: OnceLock::new(),
        }
    }

    /// The type IRI behind the `entity_type_names` key `key`.
    #[must_use]
    pub fn type_reference(&self, key: EntityTypeNameKey) -> Option<&NgsiLdReference> {
        self.type_names
            .get_or_init(|| self.types.iter().map(|(name, key)| (key, NgsiLdReference::from(name))).collect())
            .get(&key)
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

    /// The `entity_type_names` key of the entity type `name`.
    #[must_use]
    pub fn entity_type(&self, name: &EntityTypeName) -> Option<EntityTypeNameKey> {
        self.types.get(name)
    }
}

/// The names flattening an Entity's attributes needs a key for, at every
/// depth: every attribute name, and every datatype the row or element holds
/// as a key, `valueType` and `objectType`. A datatype inside a value held
/// whole in `value_object` stays in that value and needs none.
#[derive(Debug, Default)]
pub struct MentionedNames {
    /// The attribute names, unsorted and possibly repeated.
    pub attributes: Vec<AttributeName>,
    /// The `@type` IRIs, as the type names they are interned as, unsorted and
    /// possibly repeated.
    pub types: Vec<EntityTypeName>,
}

impl MentionedNames {
    /// Add the names `attributes` mentions, at every depth.
    pub fn collect(&mut self, attributes: &Attributes) {
        for (name, instances) in attributes.iter() {
            self.attributes.push(name.clone());
            for (_, instance) in instances.iter() {
                let family = instance.value();
                if let FamilyValue::Property { values, .. } = family
                    && let Some(datatype) = scalar_literal(values).and_then(|literal| literal.datatype())
                {
                    self.types.push(EntityTypeName::from(datatype));
                }
                self.types
                    .extend(family.value_type().into_iter().chain(family.object_types()).map(EntityTypeName::from));
                self.collect(instance.sub_attributes());
            }
        }
    }
}

/// The keys an Entity's rows hold at every depth: every attribute name key,
/// and every datatype, `valueType` and `objectType` key. Assembly needs the
/// name of each.
#[derive(Debug, Default)]
pub struct HeldKeys {
    /// `attribute_names` keys, unsorted and possibly repeated.
    pub attributes: Vec<AttributeNameKey>,
    /// `entity_type_names` keys, unsorted and possibly repeated.
    pub types: Vec<EntityTypeNameKey>,
}

impl HeldKeys {
    /// Add the keys one stored instance holds: its `members` and those of its
    /// `sub_attributes` elements.
    pub fn collect(&mut self, members: &InstanceMembers, sub_attributes: &[SubAttributeElement]) {
        self.members(members);
        self.collect_elements(sub_attributes);
    }

    /// The keys held, each once, in key order.
    #[must_use]
    pub fn distinct(mut self) -> HeldKeys {
        self.attributes.sort_unstable_by_key(|key| key.get());
        self.attributes.dedup();
        self.types.sort_unstable_by_key(|key| key.get());
        self.types.dedup();
        self
    }

    /// Add the keys the `sub_attributes` elements of one stored instance hold.
    pub fn collect_elements(&mut self, sub_attributes: &[SubAttributeElement]) {
        for element in sub_attributes {
            self.members(&element.members);
        }
    }

    /// Add the keys one instance's members hold.
    fn members(&mut self, members: &InstanceMembers) {
        self.attributes.push(members.attribute_name_key);
        self.types.extend(
            members
                .value
                .datatype_key()
                .into_iter()
                .chain(members.value_type_key)
                .chain(members.object_type_keys.iter().copied()),
        );
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

#[cfg(test)]
mod tests {
    use crate::name_keys::NameKeys;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference};
    use andromeda_postgresql_query::{dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};

    #[test]
    fn a_resolved_type_key_names_its_type_iri() {
        let iri = "http://www.w3.org/2001/XMLSchema#integer";
        let name = iri.parse::<EntityTypeName>().expect("a type name");
        let keys = NameKeys::new(ResolvedNames::default(), ResolvedNames::from_pairs([(name, EntityTypeNameKey::from(7))]));
        let expected = iri.parse::<NgsiLdReference>().expect("a reference");
        pretty_assertions::assert_eq!(keys.type_reference(EntityTypeNameKey::from(7)), Some(&expected));
        pretty_assertions::assert_eq!(keys.type_reference(EntityTypeNameKey::from(8)), None);
    }
}
