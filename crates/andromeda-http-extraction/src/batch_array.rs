//! Reading the array a batch request carries, item by item.
//!
//! Clauses 5.6.7 to 5.6.10 and 5.6.20 take a non-empty JSON array. Its items
//! are read as the caller asks -- the slice of text an entity is, to be
//! decoded straight from it, or the small JSON value an Entity ID is -- so a
//! batch of entities carrying large geometries is never built into one JSON
//! tree first.

use crate::{
    batch_body_refusal::{BatchBodyRefusal, BatchMembers},
    not_json_document::NotJsonDocument,
};
use serde::de::{Deserialize, DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::{fmt, marker::PhantomData};

/// Read `body` as the non-empty JSON array a batch operation requires,
/// holding `members`, each item read as a `T`.
///
/// # Errors
///
/// Returns the [`BatchBodyRefusal`] for a body that is not JSON, not an
/// array, or an empty array.
pub(crate) fn batch_items<'b, T: Deserialize<'b>>(body: &'b [u8], members: BatchMembers) -> Result<Vec<T>, BatchBodyRefusal> {
    let mut reader = JsonReader::from_slice(body);
    let read = ArraySeed::<T>(PhantomData)
        .deserialize(&mut reader)
        .and_then(|read| reader.end().map(|()| read))
        .map_err(|source| BatchBodyRefusal::NotJson(NotJsonDocument(source)))?;
    match read {
        BodyArray::Items(items) if items.is_empty() => Err(BatchBodyRefusal::Empty { members }),
        BodyArray::Items(items) => Ok(items),
        BodyArray::NotAnArray => Err(BatchBodyRefusal::NotAnArray { members }),
    }
}

/// What a batch body is at its top level.
#[derive(Debug)]
enum BodyArray<T> {
    /// An array, and its items.
    Items(Vec<T>),
    /// Any other JSON value, read to its end.
    NotAnArray,
}

/// Reads a batch body as an array of `T`.
#[derive(Debug)]
struct ArraySeed<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for ArraySeed<T> {
    type Value = BodyArray<T>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<BodyArray<T>, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for ArraySeed<T> {
    type Value = BodyArray<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON array")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<BodyArray<T>, E> {
        Ok(BodyArray::NotAnArray)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<BodyArray<T>, A::Error> {
        let mut read = Vec::new();
        while let Some(item) = items.next_element()? {
            read.push(item);
        }
        Ok(BodyArray::Items(read))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<BodyArray<T>, A::Error> {
        while members.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(BodyArray::NotAnArray)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_array::batch_items,
        batch_body_refusal::{BatchBodyRefusal, BatchMembers},
    };
    use serde_json::{Value, value::RawValue};

    #[test]
    fn an_array_body_is_read_item_by_item() {
        let items: Vec<&RawValue> = batch_items(br#"[{"id":"a"}, 7, null]"#, BatchMembers::Entities).expect("an array");
        pretty_assertions::assert_eq!(items.iter().map(|item| item.get()).collect::<Vec<_>>(), vec![r#"{"id":"a"}"#, "7", "null"]);
    }

    #[test]
    fn a_body_that_is_no_non_empty_array_is_refused_for_what_it_is() {
        assert!(matches!(
            batch_items::<Value>(b"[]", BatchMembers::EntityIds),
            Err(BatchBodyRefusal::Empty { .. })
        ));
        for body in [&br#"{"a":[1]}"#[..], b"7", b"\"x\"", b"null"] {
            assert!(matches!(
                batch_items::<Value>(body, BatchMembers::EntityIds),
                Err(BatchBodyRefusal::NotAnArray { .. })
            ));
        }
        for body in [&b"[1,"[..], b"[1] x", b"{"] {
            assert!(matches!(batch_items::<Value>(body, BatchMembers::EntityIds), Err(BatchBodyRefusal::NotJson(_))));
        }
    }

    /// Clause 5.5.4: the batch reader words a body that is no JSON as every
    /// other body reader does.
    #[test]
    fn a_body_that_is_no_json_is_named_as_every_reader_names_it() {
        let refusal = batch_items::<Value>(b"[1,", BatchMembers::Entities).expect_err("not a JSON document");
        assert!(refusal.to_string().starts_with("request body is not a JSON document:"), "{refusal}");
    }
}
