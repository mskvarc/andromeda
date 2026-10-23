//! The containers of a value read through the aliases: the elements of an
//! array, the members of an object, and an enum written as an object of one
//! member, each element or member read where it stands.

use crate::{
    aliased_deserializer::{
        Place,
        aliased_visitor::{AliasedSeed, AliasedVisitor},
    },
    core_term_aliases::{CoreTermAliases, Standing},
};
use serde::de::{DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use std::cell::Cell;

/// The elements of an array, each read through the aliases.
pub(crate) struct AliasedElements<'a, A> {
    /// The elements as written.
    elements: A,
    /// The aliases each element is read through.
    aliases: &'a CoreTermAliases,
    /// Where each element stands.
    standing: Standing,
}

impl<'a, A> AliasedElements<'a, A> {
    /// The `elements` of an array, each standing at `standing`, read through
    /// `aliases`.
    pub(crate) const fn new(elements: A, aliases: &'a CoreTermAliases, standing: Standing) -> AliasedElements<'a, A> {
        AliasedElements { elements, aliases, standing }
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for AliasedElements<'_, A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<Option<S::Value>, A::Error> {
        self.elements
            .next_element_seed(AliasedSeed::new(seed, self.aliases, Place::Value(self.standing)))
    }

    fn size_hint(&self) -> Option<usize> {
        self.elements.size_hint()
    }
}

/// The members of an object, each name read as the core term it stands for.
pub(crate) struct AliasedMembers<'a, A> {
    /// The members as written.
    members: A,
    /// The aliases each member is read through.
    aliases: &'a CoreTermAliases,
    /// Where the value of the member last named stands.
    value: Standing,
}

impl<'a, A> AliasedMembers<'a, A> {
    /// The `members` of an object, read through `aliases`.
    pub(crate) const fn new(members: A, aliases: &'a CoreTermAliases) -> AliasedMembers<'a, A> {
        AliasedMembers {
            members,
            aliases,
            value: Standing::Node,
        }
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for AliasedMembers<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
        let value = Cell::new(Standing::Node);
        let name = self.members.next_key_seed(AliasedSeed::new(seed, self.aliases, Place::Name(&value)))?;
        self.value = value.get();
        Ok(name)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.members.next_value_seed(AliasedSeed::new(seed, self.aliases, Place::Value(self.value)))
    }

    fn size_hint(&self) -> Option<usize> {
        self.members.size_hint()
    }
}

/// An enum written as an object of one member, its variant read as a member
/// name.
pub(crate) struct AliasedEnum<'a, E> {
    /// The enum as written.
    data: E,
    /// The aliases the variant and its content are read through.
    aliases: &'a CoreTermAliases,
}

impl<'a, E> AliasedEnum<'a, E> {
    /// The enum `data`, read through `aliases`.
    pub(crate) const fn new(data: E, aliases: &'a CoreTermAliases) -> AliasedEnum<'a, E> {
        AliasedEnum { data, aliases }
    }
}

impl<'a, 'de, E: EnumAccess<'de>> EnumAccess<'de> for AliasedEnum<'a, E> {
    type Error = E::Error;
    type Variant = AliasedVariant<'a, E::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, AliasedVariant<'a, E::Variant>), E::Error> {
        let value = Cell::new(Standing::Node);
        let (variant, content) = self.data.variant_seed(AliasedSeed::new(seed, self.aliases, Place::Name(&value)))?;
        Ok((
            variant,
            AliasedVariant {
                content,
                aliases: self.aliases,
                standing: value.get(),
            },
        ))
    }
}

/// The content of an enum variant, read through the aliases.
pub(crate) struct AliasedVariant<'a, C> {
    /// The content as written.
    content: C,
    /// The aliases the content is read through.
    aliases: &'a CoreTermAliases,
    /// Where the content stands.
    standing: Standing,
}

impl<'a, C> AliasedVariant<'a, C> {
    /// `visitor` reading the content through the aliases.
    const fn visitor<V>(&self, visitor: V) -> AliasedVisitor<'a, 'static, V> {
        AliasedVisitor::new(visitor, self.aliases, Place::Value(self.standing))
    }
}

impl<'de, C: VariantAccess<'de>> VariantAccess<'de> for AliasedVariant<'_, C> {
    type Error = C::Error;

    fn unit_variant(self) -> Result<(), C::Error> {
        self.content.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, C::Error> {
        let seed = AliasedSeed::new(seed, self.aliases, Place::Value(self.standing));
        self.content.newtype_variant_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, C::Error> {
        let visitor = self.visitor(visitor);
        self.content.tuple_variant(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value, C::Error> {
        let visitor = self.visitor(visitor);
        self.content.struct_variant(fields, visitor)
    }
}
