//! The visitor and the seed of a value read through the aliases: every
//! string is read where it stands, and every container is handed on read the
//! same way.

use crate::{
    aliased_deserializer::{
        AliasedDeserializer,
        Place,
        aliased_access::{AliasedElements, AliasedEnum, AliasedMembers},
    },
    core_term_aliases::CoreTermAliases,
};
use serde::de::{DeserializeSeed, Deserializer, EnumAccess, Error as DeserializeError, MapAccess, SeqAccess, Unexpected, Visitor};
use std::{borrow::Cow, fmt};

/// A visitor handed what a deserializer delivers read through the aliases.
pub(crate) struct AliasedVisitor<'a, 's, V> {
    /// The visitor of the typed resource.
    visitor: V,
    /// The aliases the value is read through.
    aliases: &'a CoreTermAliases,
    /// Where the value stands.
    place: Place<'s>,
}

impl<'a, 's, V> AliasedVisitor<'a, 's, V> {
    /// `visitor`, handed the value standing at `place` read through
    /// `aliases`.
    pub(crate) const fn new(visitor: V, aliases: &'a CoreTermAliases, place: Place<'s>) -> AliasedVisitor<'a, 's, V> {
        AliasedVisitor { visitor, aliases, place }
    }

    /// The deserializer `inner`, read at the same place through the same
    /// aliases.
    const fn nested<D>(&self, inner: D) -> AliasedDeserializer<'a, 's, D> {
        AliasedDeserializer::at(inner, self.aliases, self.place)
    }
}

/// Implements the `Visitor` methods of a value no alias applies to, each
/// handing the value to the wrapped visitor unchanged.
macro_rules! forward_to_visitor {
    ($($method:ident($value:ty)),+ $(,)?) => {
        $(
            fn $method<E: DeserializeError>(self, value: $value) -> Result<V::Value, E> {
                self.visitor.$method(value)
            }
        )+
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for AliasedVisitor<'_, '_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    forward_to_visitor!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_f32(f32),
        visit_f64(f64),
        visit_char(char),
        visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]),
        visit_byte_buf(Vec<u8>),
    );

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<V::Value, E> {
        match self.place.read(self.aliases, Cow::Borrowed(text)) {
            Cow::Borrowed(read) => self.visitor.visit_str(read),
            Cow::Owned(read) => self.visitor.visit_string(read),
        }
    }

    fn visit_borrowed_str<E: DeserializeError>(self, text: &'de str) -> Result<V::Value, E> {
        match self.place.read(self.aliases, Cow::Borrowed(text)) {
            Cow::Borrowed(read) => self.visitor.visit_borrowed_str(read),
            Cow::Owned(read) => self.visitor.visit_string(read),
        }
    }

    fn visit_string<E: DeserializeError>(self, text: String) -> Result<V::Value, E> {
        let read: Cow<'de, str> = self.place.read(self.aliases, Cow::Owned(text));
        match read {
            Cow::Borrowed(read) => self.visitor.visit_borrowed_str(read),
            Cow::Owned(read) => self.visitor.visit_string(read),
        }
    }

    // Clause 5.2.1: "None of the members described admit a null value
    // directly", so a JSON `null` is refused wherever the body holds it; a value
    // captured as JSON of its own never reaches this visitor.
    fn visit_none<E: DeserializeError>(self) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unit, &self.visitor))
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unit, &self.visitor))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        let nested = self.nested(deserializer);
        self.visitor.visit_some(nested)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        let nested = self.nested(deserializer);
        self.visitor.visit_newtype_struct(nested)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, elements: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_seq(AliasedElements::new(elements, self.aliases, self.place.elements()))
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_map(AliasedMembers::new(members, self.aliases))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_enum(AliasedEnum::new(data, self.aliases))
    }
}

/// A seed handed a deserializer that reads through the aliases.
pub(crate) struct AliasedSeed<'a, 's, S> {
    /// The seed of the typed resource.
    seed: S,
    /// The aliases the value is read through.
    aliases: &'a CoreTermAliases,
    /// Where the value stands.
    place: Place<'s>,
}

impl<'a, 's, S> AliasedSeed<'a, 's, S> {
    /// `seed`, handed the value standing at `place` read through `aliases`.
    pub(crate) const fn new(seed: S, aliases: &'a CoreTermAliases, place: Place<'s>) -> AliasedSeed<'a, 's, S> {
        AliasedSeed { seed, aliases, place }
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for AliasedSeed<'_, '_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.seed.deserialize(AliasedDeserializer::at(deserializer, self.aliases, self.place))
    }
}
