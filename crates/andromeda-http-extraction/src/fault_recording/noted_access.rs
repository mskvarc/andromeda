//! The accesses a watched visitor is handed: each reads its elements, members
//! and variants through a [`Recording`](crate::fault_recording::Recording) and
//! raises through [`Noted`].

use crate::fault_recording::{FaultSlot, RecordingSeed, noted::Noted, watching::Watching};
use serde::de::{DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};

/// The elements of an array, read watched.
pub(crate) struct NotedSeq<'s, A> {
    /// The reader's access.
    inner: A,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, A> NotedSeq<'s, A> {
    /// Read the elements `inner` holds, noting faults into `slot`.
    pub(crate) const fn new(inner: A, slot: &'s FaultSlot) -> NotedSeq<'s, A> {
        NotedSeq { inner, slot }
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for NotedSeq<'_, A> {
    type Error = Noted<A::Error>;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Noted<A::Error>> {
        self.inner.next_element_seed(RecordingSeed::new(seed, self.slot)).map_err(Noted::passed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// The members of an object, read watched.
pub(crate) struct NotedMap<'s, A> {
    /// The reader's access.
    inner: A,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, A> NotedMap<'s, A> {
    /// Read the members `inner` holds, noting faults into `slot`.
    pub(crate) const fn new(inner: A, slot: &'s FaultSlot) -> NotedMap<'s, A> {
        NotedMap { inner, slot }
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for NotedMap<'_, A> {
    type Error = Noted<A::Error>;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Noted<A::Error>> {
        self.inner.next_key_seed(RecordingSeed::new(seed, self.slot)).map_err(Noted::passed)
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, Noted<A::Error>> {
        self.inner.next_value_seed(RecordingSeed::new(seed, self.slot)).map_err(Noted::passed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// The variant an enum names, read watched.
pub(crate) struct NotedEnum<'s, A> {
    /// The reader's access.
    inner: A,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'s, A> NotedEnum<'s, A> {
    /// Read the variant `inner` names, noting faults into `slot`.
    pub(crate) const fn new(inner: A, slot: &'s FaultSlot) -> NotedEnum<'s, A> {
        NotedEnum { inner, slot }
    }
}

impl<'de, 's, A: EnumAccess<'de>> EnumAccess<'de> for NotedEnum<'s, A> {
    type Error = Noted<A::Error>;
    type Variant = NotedVariant<'s, A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<(T::Value, NotedVariant<'s, A::Variant>), Noted<A::Error>> {
        let slot = self.slot;
        self.inner
            .variant_seed(RecordingSeed::new(seed, slot))
            .map(|(value, variant)| (value, NotedVariant { inner: variant, slot }))
            .map_err(Noted::passed)
    }
}

/// The content of an enum's variant, read watched.
pub(crate) struct NotedVariant<'s, A> {
    /// The reader's access.
    inner: A,
    /// Where faults are noted.
    slot: &'s FaultSlot,
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for NotedVariant<'_, A> {
    type Error = Noted<A::Error>;

    fn unit_variant(self) -> Result<(), Noted<A::Error>> {
        self.inner.unit_variant().map_err(Noted::passed)
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Noted<A::Error>> {
        self.inner.newtype_variant_seed(RecordingSeed::new(seed, self.slot)).map_err(Noted::passed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Noted<A::Error>> {
        self.inner.tuple_variant(len, Watching::new(visitor, self.slot)).map_err(Noted::passed)
    }

    fn struct_variant<V: Visitor<'de>>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value, Noted<A::Error>> {
        self.inner.struct_variant(fields, Watching::new(visitor, self.slot)).map_err(Noted::passed)
    }
}
