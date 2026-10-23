//! Serde support for an update-fragment member whose data type is an array.
//!
//! Clause 4.5.0 spells the NGSI-LD Null "an array with one single NGSI-LD
//! Null" where an array is required; the bare URI deletes such a member too,
//! as it deletes a member of any other data type. The Null beside another
//! element is no deletion: it is an element, read as the member's elements
//! are. A deleted member is written back in the array form, which a reader
//! that takes the array alone also reads.

use crate::clearable::Clearable;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::{NULL_URI, is_ngsi_ld_null};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{
        Error as DeserializeError,
        SeqAccess,
        Unexpected,
        Visitor,
        value::{BorrowedStrDeserializer, SeqAccessDeserializer},
    },
};
use std::{fmt, marker::PhantomData};

/// Deserialize an array member: the NGSI-LD Null, bare or as the one element
/// of an array, deletes it; any other array is read element by element.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither an array
/// nor the Null, a JSON `null` included, and an element's refusal otherwise.
pub fn deserialize<'de, D, E>(deserializer: D) -> Result<Clearable<Vec<E>>, D::Error>
where
    D: Deserializer<'de>,
    E: Deserialize<'de>,
{
    deserializer.deserialize_any(ArrayVisitor(PhantomData))
}

/// Serialize an array member as the fragment wrote it: the elements, or the
/// array holding the NGSI-LD Null alone for a deleted member.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S, E>(member: &Clearable<Vec<E>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    E: Serialize,
{
    match member {
        Clearable::Set(elements) => elements.serialize(serializer),
        Clearable::NgsiLdNull => [NULL_URI.as_str()].serialize(serializer),
        Clearable::Absent => serializer.serialize_none(),
    }
}

/// Reads an array member.
struct ArrayVisitor<E>(PhantomData<E>);

impl<'de, E: Deserialize<'de>> Visitor<'de> for ArrayVisitor<E> {
    type Value = Clearable<Vec<E>>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array or the NGSI-LD Null \"urn:ngsi-ld:null\"")
    }

    fn visit_str<Er: DeserializeError>(self, text: &str) -> Result<Clearable<Vec<E>>, Er> {
        if is_ngsi_ld_null(text) {
            return Ok(Clearable::NgsiLdNull);
        }
        Err(Er::invalid_type(Unexpected::Str(text), &self))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, elements: A) -> Result<Clearable<Vec<E>>, A::Error> {
        let elements = Vec::<Clearable<E>>::deserialize(SeqAccessDeserializer::new(elements))?;
        if matches!(elements.as_slice(), [Clearable::NgsiLdNull]) {
            return Ok(Clearable::NgsiLdNull);
        }
        elements.into_iter().map(element).collect::<Result<Vec<E>, A::Error>>().map(Clearable::Set)
    }

    // Clause 4.5.0: "null cannot be used as a value in NGSI-LD".
    fn visit_unit<Er: DeserializeError>(self) -> Result<Clearable<Vec<E>>, Er> {
        Err(Er::invalid_type(Unexpected::Unit, &self))
    }

    fn visit_none<Er: DeserializeError>(self) -> Result<Clearable<Vec<E>>, Er> {
        Err(Er::invalid_type(Unexpected::Unit, &self))
    }
}

/// One element of an array that is no deletion: the NGSI-LD Null beside other
/// elements is read as an element spelled that way.
fn element<'de, E: Deserialize<'de>, Er: DeserializeError>(read: Clearable<E>) -> Result<E, Er> {
    match read {
        Clearable::Set(element) => Ok(element),
        Clearable::NgsiLdNull | Clearable::Absent => E::deserialize(BorrowedStrDeserializer::new(NULL_URI.as_str())),
    }
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, clearable_array};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(
            default,
            skip_serializing_if = "Clearable::is_absent",
            deserialize_with = "clearable_array::deserialize",
            serialize_with = "clearable_array::serialize"
        )]
        names: Clearable<Vec<String>>,
        #[serde(
            default,
            skip_serializing_if = "Clearable::is_absent",
            deserialize_with = "clearable_array::deserialize",
            serialize_with = "clearable_array::serialize"
        )]
        numbers: Clearable<Vec<u8>>,
    }

    fn read(fragment: &serde_json::Value) -> Result<Carrier, serde_json::Error> {
        serde_json::from_str(&fragment.to_string())
    }

    /// Clause 4.5.0: an array member is deleted by an array of one NGSI-LD Null
    /// and by the bare Null, and is written back in the array form.
    #[test]
    fn an_array_member_is_deleted_by_the_null_alone_or_bare() {
        for deletion in [json!(["urn:ngsi-ld:null"]), json!("urn:ngsi-ld:null")] {
            let deleted = read(&json!({"names": deletion.clone(), "numbers": deletion.clone()})).expect("the Null"); // clone: the deletion is written into both members and named below
            assert!(deleted.names.is_ngsi_ld_null() && deleted.numbers.is_ngsi_ld_null(), "{deletion}");
            pretty_assertions::assert_eq!(
                serde_json::to_value(&deleted).expect("writes"),
                json!({"names": ["urn:ngsi-ld:null"], "numbers": ["urn:ngsi-ld:null"]})
            );
        }
    }

    /// The Null beside another element is an element.
    #[test]
    fn the_null_beside_another_element_is_an_element() {
        let read = read(&json!({"names": ["speed", "urn:ngsi-ld:null"]})).expect("two names");
        pretty_assertions::assert_eq!(read.names, Clearable::Set(vec!["speed".to_owned(), "urn:ngsi-ld:null".to_owned()]));
        pretty_assertions::assert_eq!(serde_json::to_value(&read).expect("writes"), json!({"names": ["speed", "urn:ngsi-ld:null"]}));
    }

    #[test]
    fn an_array_is_read_element_by_element() {
        let read = read(&json!({"numbers": [1, 2]})).expect("two numbers");
        pretty_assertions::assert_eq!(read.numbers, Clearable::Set(vec![1, 2]));
    }

    /// Clause 4.5.0: a JSON `null` is no value, as the member or as an element.
    #[test]
    fn a_json_null_is_refused_as_the_member_and_as_an_element() {
        for fragment in [json!({"names": null}), json!({"names": [null]}), json!({"names": ["a", null]})] {
            assert!(read(&fragment).is_err(), "{fragment}");
        }
    }

    #[test]
    fn a_member_that_is_no_array_is_refused() {
        for fragment in [
            json!({"names": "speed"}),
            json!({"names": 5}),
            json!({"numbers": ["urn:ngsi-ld:null", "x"]}),
            json!({"numbers": {"a": 1}}),
        ] {
            assert!(read(&fragment).is_err(), "{fragment}");
        }
    }
}
