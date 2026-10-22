//! Recording the expanded `coordinates` of a geometry onto a coordinate tape
//! (clause 4.7, IETF RFC 7946 section 3.1).
//!
//! Expansion under the core context turns `coordinates` into nested `@list`
//! objects whose leaves are `{"@value": n}` ordinates, the whole wrapped in
//! the one-element array of an expanded member. Each list is recorded as an
//! array of the tape and each numeric `@value` as an ordinate, whatever came
//! first among the node's members, so the geometry crate's one reader decides
//! how the coordinates nest once the type is known, with the refusals every
//! other geometry reading raises. Anything else is recorded as a member that is
//! no ordinate and refused where it stands.

use crate::codec::{object_key::ObjectKey, raw_scalar::RawScalar};
use andromeda_ngsi_ld_geometry::coordinate_tape::CoordinateTape;
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Number;
use std::fmt;

/// Records the `coordinates` member of a geometry node, `[{"@list": [...]}]`.
///
/// A member holding anything but one item is recorded as a single member that
/// is no array, which the reader refuses at the root.
pub(crate) struct CoordinatesSeed;

impl<'de> DeserializeSeed<'de> for CoordinatesSeed {
    type Value = CoordinateTape;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<CoordinateTape, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CoordinatesSeed {
    type Value = CoordinateTape;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded coordinates member")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<CoordinateTape, A::Error> {
        let mut tape = CoordinateTape::default();
        let recorded = items.next_element_seed(ItemSeed { tape: &mut tape })?.is_some();
        let beside = items.next_element::<IgnoredAny>()?.is_some();
        while items.next_element::<IgnoredAny>()?.is_some() {}
        if recorded && !beside {
            return Ok(tape);
        }
        Ok(not_an_array())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CoordinateTape, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(not_an_array())
    }

    fn visit_bool<E>(self, _: bool) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }

    fn visit_str<E>(self, _: &str) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }

    fn visit_unit<E>(self) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }

    fn visit_u64<E>(self, _: u64) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }

    fn visit_i64<E>(self, _: i64) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }

    fn visit_f64<E>(self, _: f64) -> Result<CoordinateTape, E> {
        Ok(not_an_array())
    }
}

/// A tape holding one member that is no array.
fn not_an_array() -> CoordinateTape {
    let mut tape = CoordinateTape::default();
    tape.other();
    tape
}

/// Records one item of an expanded coordinate list: a nested `@list` as an
/// array, a numeric `@value` as an ordinate, anything else as a member that is
/// no ordinate.
struct ItemSeed<'t> {
    /// The tape the item is recorded onto.
    tape: &'t mut CoordinateTape,
}

impl<'de> DeserializeSeed<'de> for ItemSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ItemSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded coordinate list item")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut recorded = false;
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::List if !recorded => {
                    map.next_value_seed(ListSeed { tape: &mut *self.tape })?;
                    recorded = true;
                }
                ObjectKey::Value if !recorded => {
                    let ordinate = map.next_value::<RawScalar<'de>>()?.as_f64();
                    self.tape.number(ordinate);
                    recorded = true;
                }
                ObjectKey::List
                | ObjectKey::Value
                | ObjectKey::Id
                | ObjectKey::Type
                | ObjectKey::Language
                | ObjectKey::Direction
                | ObjectKey::OtherKeyword
                | ObjectKey::Name(_)
                | ObjectKey::InvalidName(_) => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        if !recorded {
            self.tape.other();
        }
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        self.tape.other();
        Ok(())
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_u64<E>(self, value: u64) -> Result<(), E> {
        self.tape.number(Number::from(value).as_f64());
        Ok(())
    }

    fn visit_i64<E>(self, value: i64) -> Result<(), E> {
        self.tape.number(Number::from(value).as_f64());
        Ok(())
    }

    fn visit_f64<E>(self, value: f64) -> Result<(), E> {
        self.tape.number(Some(value));
        Ok(())
    }
}

/// Records the items of one `@list` as an array of the tape.
struct ListSeed<'t> {
    /// The tape the list is recorded onto.
    tape: &'t mut CoordinateTape,
}

impl<'de> DeserializeSeed<'de> for ListSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ListSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the items of an expanded coordinate list")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        self.tape.open();
        while items.next_element_seed(ItemSeed { tape: &mut *self.tape })?.is_some() {}
        self.tape.close();
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        self.tape.other();
        Ok(())
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        self.tape.other();
        Ok(())
    }
}
