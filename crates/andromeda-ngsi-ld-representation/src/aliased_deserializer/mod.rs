//! A request body decoded under the core terms its member names alias, with
//! no JSON `null` in it.
//!
//! [`CoreTermAliases`] is the one rule for a core term written under another
//! term; [`AliasedNode`](crate::compact_node::aliased_node::AliasedNode)
//! applies it to a node walked in place, and [`AliasedDeserializer`] applies
//! it to a body decoded straight from its text into a typed resource. At every
//! depth each member name is read as the core term it stands for, and each
//! string of a `type` member as the class it names. A JSON `null` at any depth
//! is refused: clause 5.2.1 admits no null value for a member of any data
//! type, and an update deletes a member with the NGSI-LD Null instead (clause
//! 4.5.0). A value the resource captures as JSON of its own (a `JsonLiteral`,
//! a `serde_json` raw value) is handed over as written: what a document holds
//! as JSON is never renamed inside, and may hold a `null` as data.

mod aliased_access;
mod aliased_visitor;

use crate::{
    aliased_deserializer::aliased_visitor::AliasedVisitor,
    core_term_aliases::{CoreTermAliases, Standing},
};
use andromeda_json_literal::LITERAL_CAPTURE;
use serde::de::{Deserializer, Visitor};
use std::{borrow::Cow, cell::Cell};

/// The newtype name `serde_json` captures a raw value under, which a raw
/// value asks its deserializer for. `serde_json` does not export the name.
const RAW_VALUE_CAPTURE: &str = "$serde_json::private::RawValue";

/// Where the text a deserializer delivers stands.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Place<'s> {
    /// A value, standing where the standing says.
    Value(Standing),
    /// A member name, whose core term decides where the member's value
    /// stands.
    Name(&'s Cell<Standing>),
}

impl Place<'_> {
    /// The string `text`, written at this place, read through `aliases`.
    pub(crate) fn read<'n>(self, aliases: &CoreTermAliases, text: Cow<'n, str>) -> Cow<'n, str> {
        match self {
            Place::Value(standing) => aliases.string(text, standing),
            Place::Name(value) => {
                let name = aliases.core_name(text);
                value.set(Standing::of_member(&name));
                name
            }
        }
    }

    /// Where the elements of an array written at this place stand: the
    /// strings of an array a `type` member holds name classes as one string
    /// would.
    pub(crate) const fn elements(self) -> Standing {
        match self {
            Place::Value(standing) => standing,
            Place::Name(_) => Standing::Node,
        }
    }
}

/// A deserializer whose member names are read under the core terms they
/// alias.
#[derive(Debug)]
pub struct AliasedDeserializer<'a, 's, D> {
    /// The deserializer of the text as written.
    inner: D,
    /// The aliases the text is read through.
    aliases: &'a CoreTermAliases,
    /// Where the value it delivers stands.
    place: Place<'s>,
}

impl<'a, D> AliasedDeserializer<'a, 'static, D> {
    /// Read the document `inner` delivers through `aliases`.
    #[must_use]
    pub const fn new(inner: D, aliases: &'a CoreTermAliases) -> AliasedDeserializer<'a, 'static, D> {
        AliasedDeserializer {
            inner,
            aliases,
            place: Place::Value(Standing::Node),
        }
    }
}

impl<'a, 's, D> AliasedDeserializer<'a, 's, D> {
    /// Read the value `inner` delivers, standing at `place`, through
    /// `aliases`.
    pub(crate) const fn at(inner: D, aliases: &'a CoreTermAliases, place: Place<'s>) -> AliasedDeserializer<'a, 's, D> {
        AliasedDeserializer { inner, aliases, place }
    }

    /// The deserializer as written, and `visitor` reading what it delivers
    /// through the aliases.
    fn split<V>(self, visitor: V) -> (D, AliasedVisitor<'a, 's, V>) {
        let AliasedDeserializer { inner, aliases, place } = self;
        (inner, AliasedVisitor::new(visitor, aliases, place))
    }
}

/// Implements the `Deserializer` methods that take only a visitor, each
/// asking the deserializer as written with the visitor wrapped.
macro_rules! forward_to_inner {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
                let (inner, visitor) = self.split(visitor);
                inner.$method(visitor)
            }
        )+
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for AliasedDeserializer<'_, '_, D> {
    type Error = D::Error;

    forward_to_inner!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
    );

    // A member the resource does not read is skipped as written: no name in it
    // is read, and `serde_json` reports every skipped value as a unit, which is
    // no `null` the body holds.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_ignored_any(visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value, D::Error> {
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value, D::Error> {
        if name == LITERAL_CAPTURE || name == RAW_VALUE_CAPTURE {
            return self.inner.deserialize_newtype_struct(name, visitor);
        }
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_newtype_struct(name, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, D::Error> {
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_tuple(len, visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, name: &'static str, len: usize, visitor: V) -> Result<V::Value, D::Error> {
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_tuple_struct(name, len, visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(self, name: &'static str, fields: &'static [&'static str], visitor: V) -> Result<V::Value, D::Error> {
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(self, name: &'static str, variants: &'static [&'static str], visitor: V) -> Result<V::Value, D::Error> {
        let (inner, visitor) = self.split(visitor);
        inner.deserialize_enum(name, variants, visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
