//! The streaming seeds a geometry encoded as a JSON string is counted
//! through, building nothing from its text.

use crate::geometry_census::{COORDINATES_MEMBER, census::Census};
use andromeda_ngsi_ld_number::json_value::value_census::{MemberName, MemberNameSeed, ValueCensusSeed};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// Reads a geometry text: an object's `coordinates` as coordinates and its
/// other members as values, and anything that is no object as values.
#[derive(Clone, Copy)]
pub(crate) struct GeometryTextSeed<'c, 'a> {
    /// The census the geometry is counted into.
    census: &'c Census<'a>,
}

impl<'c, 'a> GeometryTextSeed<'c, 'a> {
    /// A seed counting a geometry text into `census`.
    pub(crate) const fn new(census: &'c Census<'a>) -> GeometryTextSeed<'c, 'a> {
        GeometryTextSeed { census }
    }

    /// The seed counting a value that is no geometry object.
    const fn values(self) -> ValueCensusSeed<'a> {
        ValueCensusSeed::new(self.census.values)
    }
}

impl<'de> DeserializeSeed<'de> for GeometryTextSeed<'_, '_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for GeometryTextSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GeoJSON geometry object")
    }

    fn visit_bool<E: DeserializeError>(self, value: bool) -> Result<(), E> {
        self.values().visit_bool(value)
    }

    fn visit_i64<E: DeserializeError>(self, value: i64) -> Result<(), E> {
        self.values().visit_i64(value)
    }

    fn visit_u64<E: DeserializeError>(self, value: u64) -> Result<(), E> {
        self.values().visit_u64(value)
    }

    fn visit_f64<E: DeserializeError>(self, value: f64) -> Result<(), E> {
        self.values().visit_f64(value)
    }

    fn visit_str<E: DeserializeError>(self, value: &str) -> Result<(), E> {
        self.values().visit_str(value)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        self.values().visit_unit()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<(), A::Error> {
        self.values().visit_seq(items)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        self.census.values.count()?;
        while let Some(member) = members.next_key_seed(GeometryMemberSeed)? {
            match member {
                GeometryMember::Coordinates => members.next_value_seed(NodeSeed { census: self.census })?,
                GeometryMember::Other => members.next_value_seed(self.values())?,
            }
        }
        Ok(())
    }
}

/// A member of a geometry object, as far as the census cares.
enum GeometryMember {
    /// The `coordinates` member.
    Coordinates,
    /// Any other member.
    Other,
}

/// Reads one member name of a geometry object without keeping it.
struct GeometryMemberSeed;

impl<'de> DeserializeSeed<'de> for GeometryMemberSeed {
    type Value = GeometryMember;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeometryMember, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for GeometryMemberSeed {
    type Value = GeometryMember;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<GeometryMember, E> {
        if name == COORDINATES_MEMBER {
            Ok(GeometryMember::Coordinates)
        } else {
            Ok(GeometryMember::Other)
        }
    }
}

/// Reads one coordinate node: an array is a list or a position, anything else
/// is counted as values.
#[derive(Clone, Copy)]
pub(crate) struct NodeSeed<'c, 'a> {
    /// The census the node is counted into.
    census: &'c Census<'a>,
}

impl<'c, 'a> NodeSeed<'c, 'a> {
    /// A seed counting a coordinate node into `census`.
    pub(crate) const fn new(census: &'c Census<'a>) -> NodeSeed<'c, 'a> {
        NodeSeed { census }
    }

    /// The seed counting a node that is no array.
    const fn values(self) -> ValueCensusSeed<'a> {
        ValueCensusSeed::new(self.census.values)
    }
}

impl<'de> DeserializeSeed<'de> for NodeSeed<'_, '_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NodeSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeoJSON coordinates")
    }

    fn visit_bool<E: DeserializeError>(self, value: bool) -> Result<(), E> {
        self.values().visit_bool(value)
    }

    fn visit_i64<E: DeserializeError>(self, value: i64) -> Result<(), E> {
        self.values().visit_i64(value)
    }

    fn visit_u64<E: DeserializeError>(self, value: u64) -> Result<(), E> {
        self.values().visit_u64(value)
    }

    fn visit_f64<E: DeserializeError>(self, value: f64) -> Result<(), E> {
        self.values().visit_f64(value)
    }

    fn visit_str<E: DeserializeError>(self, value: &str) -> Result<(), E> {
        self.values().visit_str(value)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        self.values().visit_unit()
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<(), A::Error> {
        self.values().visit_map(members)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        match items.next_element_seed(FirstMemberSeed { census: self.census })? {
            None => self.census.values.count(),
            Some(FirstMember::Array) => {
                self.census.values.count()?;
                while items.next_element_seed(self)?.is_some() {}
                Ok(())
            }
            Some(FirstMember::NoArray) => {
                self.census.stream_position()?;
                while items.next_element_seed(OrdinateSeed { census: self.census })?.is_some() {}
                Ok(())
            }
        }
    }
}

/// What the first member of a coordinate array is, which decides whether the
/// array is a list or a position.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstMember {
    /// An array, already counted as a node: its parent is a list.
    Array,
    /// Anything else, already counted as an ordinate: its parent is a
    /// position.
    NoArray,
}

/// Reads the first member of a coordinate array.
#[derive(Clone, Copy)]
struct FirstMemberSeed<'c, 'a> {
    /// The census the member is counted into.
    census: &'c Census<'a>,
}

impl<'de> DeserializeSeed<'de> for FirstMemberSeed<'_, '_> {
    type Value = FirstMember;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<FirstMember, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for FirstMemberSeed<'_, '_> {
    type Value = FirstMember;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a coordinate list or ordinate")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<FirstMember, E> {
        Ok(FirstMember::NoArray)
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<FirstMember, A::Error> {
        OrdinateSeed { census: self.census }.visit_map(members)?;
        Ok(FirstMember::NoArray)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<FirstMember, A::Error> {
        NodeSeed { census: self.census }.visit_seq(items)?;
        Ok(FirstMember::Array)
    }
}

/// Reads one member of a position after its first: an ordinate is part of the
/// position, anything that holds values counts them.
#[derive(Clone, Copy)]
struct OrdinateSeed<'c, 'a> {
    /// The census the member is counted into.
    census: &'c Census<'a>,
}

impl<'de> DeserializeSeed<'de> for OrdinateSeed<'_, '_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for OrdinateSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an ordinate")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<(), A::Error> {
        ValueCensusSeed::new(self.census.values).visit_seq(items)
    }

    // With `arbitrary_precision` the reader hands a decimal ordinate over as a
    // one-member map holding its token: an ordinate like any other. A real
    // object is counted as the values it holds.
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        match members.next_key_seed(MemberNameSeed)? {
            None => self.census.values.count(),
            Some(MemberName::NumberToken) => {
                members.next_value::<IgnoredAny>()?;
                Ok(())
            }
            Some(MemberName::Member) => ValueCensusSeed::new(self.census.values).count_object(members),
        }
    }
}
