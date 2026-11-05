//! Reading one member of a compact `GeoJSON` geometry object: its name into
//! the member it names, and its value into what that member says.
//!
//! `coordinates` and `bbox` go onto a [`CoordinateTape`] each, a foreign member
//! (IETF RFC 7946 section 6.1) into the JSON value it holds, `type` and
//! `geometries` into the shape of their value, read to its end.

use crate::{
    coordinate_tape::{CoordinateTape, CoordinateTapeSeed},
    foreign_members::ForeignMemberName,
    geometry_census::GeometryPositionLimit,
    geometry_member::GeometryMember,
    geometry_members::{ForeignMemberReading, GeometriesMember, GeometryMembers, JsonLdKeywords, TypeMember},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_number::json_value::value_census::{MemberName, MemberNameSeed};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::{fmt, str::FromStr};

/// Read the value of the member `key` into `members`, keeping a foreign member
/// only when `foreign` says so.
pub(crate) fn read_member<'de, A: MapAccess<'de>>(
    map: &mut A,
    key: GeometryKey,
    foreign: ForeignMemberReading,
    positions: Option<GeometryPositionLimit>,
    members: &mut GeometryMembers,
) -> Result<(), A::Error> {
    match key {
        GeometryKey::Type => {
            members.geometry_type = match map.next_value_seed(MemberShapeSeed)? {
                MemberShape::Text(token) => TypeMember::of(&token),
                MemberShape::Number | MemberShape::Array | MemberShape::Other => TypeMember::Missing,
            };
        }
        GeometryKey::Coordinates => members.coordinates = Some(read_tape(map, positions)?),
        GeometryKey::Bbox => members.bbox = Some(read_tape(map, None)?),
        GeometryKey::Geometries => {
            members.geometries = match map.next_value_seed(MemberShapeSeed)? {
                MemberShape::Array => GeometriesMember::Array,
                MemberShape::Text(_) | MemberShape::Number | MemberShape::Other => GeometriesMember::Absent,
            };
        }
        GeometryKey::Other(name) => match (ForeignMemberName::try_from(&*name), foreign) {
            (Ok(foreign_name), ForeignMemberReading::Kept) => {
                members.foreign.insert(foreign_name, map.next_value::<JsonLiteral>()?);
            }
            (Ok(_), ForeignMemberReading::Skipped) => {
                map.next_value::<IgnoredAny>()?;
            }
            (Err(_), ForeignMemberReading::Kept | ForeignMemberReading::Skipped) => {
                members.json_ld_keywords = members.json_ld_keywords.and(JsonLdKeywords::of(&name));
                map.next_value::<IgnoredAny>()?;
            }
        },
    }
    Ok(())
}

/// Record the value of the current member onto a tape.
fn read_tape<'de, A: MapAccess<'de>>(map: &mut A, positions: Option<GeometryPositionLimit>) -> Result<CoordinateTape, A::Error> {
    let mut tape = positions.map_or_else(CoordinateTape::default, CoordinateTape::within);
    map.next_value_seed(CoordinateTapeSeed::new(&mut tape))?;
    Ok(tape)
}

/// The member of a geometry object a member name names.
#[derive(Debug)]
pub(crate) enum GeometryKey {
    /// `type`.
    Type,
    /// `coordinates`.
    Coordinates,
    /// `bbox`.
    Bbox,
    /// `geometries`.
    Geometries,
    /// Any other member, by name.
    Other(Box<str>),
}

/// Reads one member name of a geometry object.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeometryKeySeed;

impl<'de> DeserializeSeed<'de> for GeometryKeySeed {
    type Value = GeometryKey;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeometryKey, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for GeometryKeySeed {
    type Value = GeometryKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<GeometryKey, E> {
        Ok(match GeometryMember::from_str(name) {
            Ok(GeometryMember::Type) => GeometryKey::Type,
            Ok(GeometryMember::Coordinates) => GeometryKey::Coordinates,
            Ok(GeometryMember::Bbox) => GeometryKey::Bbox,
            Ok(GeometryMember::Geometries) => GeometryKey::Geometries,
            Err(_) => GeometryKey::Other(Box::from(name)),
        })
    }
}

/// What a member value is, as far as a geometry object's `type` and
/// `geometries` ask.
#[derive(Debug)]
enum MemberShape {
    /// A string, and its text.
    Text(String),
    /// A number.
    Number,
    /// An array.
    Array,
    /// A boolean, null, or an object.
    Other,
}

/// Reads one member value into its [`MemberShape`], to its end.
#[derive(Clone, Copy, Debug)]
struct MemberShapeSeed;

impl<'de> DeserializeSeed<'de> for MemberShapeSeed {
    type Value = MemberShape;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MemberShape, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for MemberShapeSeed {
    type Value = MemberShape;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_i128<E: DeserializeError>(self, _value: i128) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_u128<E: DeserializeError>(self, _value: u128) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<MemberShape, E> {
        Ok(MemberShape::Number)
    }

    fn visit_str<E: DeserializeError>(self, value: &str) -> Result<MemberShape, E> {
        Ok(MemberShape::Text(value.to_owned()))
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_none<E: DeserializeError>(self) -> Result<MemberShape, E> {
        Ok(MemberShape::Other)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<MemberShape, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(MemberShape::Array)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<MemberShape, A::Error> {
        match members.next_key_seed(MemberNameSeed)? {
            // The reader's spelling of a number that is not a 64-bit integer.
            Some(MemberName::NumberToken) => {
                members.next_value::<IgnoredAny>()?;
                Ok(MemberShape::Number)
            }
            Some(MemberName::Member) => {
                members.next_value::<IgnoredAny>()?;
                while members.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MemberShape::Other)
            }
            None => Ok(MemberShape::Other),
        }
    }
}
