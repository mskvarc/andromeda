//! A visitor whose refusals are raised through
//! [`Noted`](crate::fault_recording::noted::Noted) and settled into a
//! [`FaultSlot`].

use crate::fault_recording::{
    FaultSlot,
    Recording,
    noted_access::{NotedEnum, NotedMap, NotedSeq},
};
use serde::de::{Deserializer, EnumAccess, Error as DecodeError, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// The visitor of a decoding, watched for the faults it raises.
pub(crate) struct Watching<'s, V> {
    /// The visitor of the decoding.
    inner: V,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, V> Watching<'s, V> {
    /// Watch `inner`, noting faults into `slot`.
    pub(crate) const fn new(inner: V, slot: &'s FaultSlot) -> Watching<'s, V> {
        Watching { inner, slot }
    }
}

/// Forwards each named `visit_*` method that takes one value to the inner
/// visitor, raising its refusals through
/// [`Noted`](crate::fault_recording::noted::Noted).
macro_rules! forward_value {
    ($($method:ident($value:ty)),* $(,)?) => {$(
        fn $method<E: DecodeError>(self, value: $value) -> Result<V::Value, E> {
            self.slot.settle(self.inner.$method(value))
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Watching<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    forward_value!(
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
        visit_str(&str),
        visit_borrowed_str(&'de str),
        visit_string(String),
        visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]),
        visit_byte_buf(Vec<u8>),
    );

    fn visit_none<E: DecodeError>(self) -> Result<V::Value, E> {
        self.slot.settle(self.inner.visit_none())
    }

    fn visit_unit<E: DecodeError>(self) -> Result<V::Value, E> {
        self.slot.settle(self.inner.visit_unit())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.inner.visit_some(Recording::new(deserializer, self.slot))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.inner.visit_newtype_struct(Recording::new(deserializer, self.slot))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        self.slot.settle(self.inner.visit_seq(NotedSeq::new(seq, self.slot)))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.slot.settle(self.inner.visit_map(NotedMap::new(map, self.slot)))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.slot.settle(self.inner.visit_enum(NotedEnum::new(data, self.slot)))
    }
}
