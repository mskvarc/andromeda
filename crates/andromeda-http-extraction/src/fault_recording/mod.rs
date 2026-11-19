//! A decoding that notes the typed fault a derived decoding raised, beside the
//! failure it returns.
//!
//! A derived decoding raises its own refusals through the error type of the
//! deserializer it reads from, which keeps nothing of them but their text.
//! [`Recording`] stands between a reader and every visitor and access the
//! decoding hands it, so a refusal is raised through [`Noted`] instead: the
//! reader still receives its own error, word for word, and the [`FaultSlot`]
//! keeps the [`DecodeFault`] it stood for. A decoding that fails reports the
//! last fault noted only when that fault is the failure it returned.

mod noted;
mod noted_access;
mod watching;

use crate::{
    decode_fault::DecodeFault,
    fault_recording::{noted::Noted, watching::Watching},
};
use andromeda_ngsi_ld_representation::decode_message::DecodeMessage;
use serde::de::{DeserializeSeed, Deserializer, Visitor};
use std::{cell::Cell, fmt::Display};

/// Where a decoding notes the last fault its declaration raised.
#[derive(Default)]
pub(crate) struct FaultSlot(Cell<Option<NotedFault>>);

/// A fault, and the text the reader's own error gave it.
struct NotedFault {
    /// The fault.
    fault: DecodeFault,
    /// The reader's text for it, before the reader counted a position in.
    message: String,
}

impl FaultSlot {
    /// The outcome a visitor returned, with the fault it raised noted and the
    /// reader's own error handed back.
    fn settle<T, E: Display>(&self, outcome: Result<T, Noted<E>>) -> Result<T, E> {
        outcome.map_err(|Noted { inner, fault }| {
            if let Some(fault) = fault {
                self.0.set(Some(NotedFault {
                    message: inner.to_string(),
                    fault,
                }));
            }
            inner
        })
    }

    /// The fault last noted, when it is the one `failure` reports.
    pub(crate) fn fault_reported_by(self, failure: &serde_json::Error) -> Option<DecodeFault> {
        let reported = DecodeMessage(failure).to_string();
        self.0.into_inner().filter(|noted| noted.message == reported).map(|noted| noted.fault)
    }
}

/// A deserializer that hands every visitor it is given to the decoding as a
/// [`Watching`] one, noting into `slot`.
pub(crate) struct Recording<'s, D> {
    /// The deserializer read from.
    inner: D,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, D> Recording<'s, D> {
    /// Read from `inner`, noting faults into `slot`.
    pub(crate) const fn new(inner: D, slot: &'s FaultSlot) -> Recording<'s, D> {
        Recording { inner, slot }
    }
}

/// Forwards each named `deserialize_*` method that takes a visitor alone to
/// the inner deserializer, with the visitor watched.
macro_rules! forward_watched {
    ($($method:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
            self.inner.$method(Watching::new(visitor, self.slot))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Recording<'_, D> {
    type Error = D::Error;

    forward_watched!(
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
        deserialize_ignored_any,
    );

    fn deserialize_unit_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_unit_struct(name, Watching::new(visitor, self.slot))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, name: &'static str, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_newtype_struct(name, Watching::new(visitor, self.slot))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_tuple(len, Watching::new(visitor, self.slot))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, name: &'static str, len: usize, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_tuple_struct(name, len, Watching::new(visitor, self.slot))
    }

    fn deserialize_struct<V: Visitor<'de>>(self, name: &'static str, fields: &'static [&'static str], visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_struct(name, fields, Watching::new(visitor, self.slot))
    }

    fn deserialize_enum<V: Visitor<'de>>(self, name: &'static str, variants: &'static [&'static str], visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_enum(name, variants, Watching::new(visitor, self.slot))
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// A seed whose deserializer is read through a [`Recording`].
pub(crate) struct RecordingSeed<'s, T> {
    /// The seed of the decoding.
    inner: T,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, T> RecordingSeed<'s, T> {
    /// Seed `inner`, noting faults into `slot`.
    pub(crate) const fn new(inner: T, slot: &'s FaultSlot) -> RecordingSeed<'s, T> {
        RecordingSeed { inner, slot }
    }
}

impl<'de, T: DeserializeSeed<'de>> DeserializeSeed<'de> for RecordingSeed<'_, T> {
    type Value = T::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T::Value, D::Error> {
        self.inner.deserialize(Recording::new(deserializer, self.slot))
    }
}

#[cfg(test)]
mod tests;
