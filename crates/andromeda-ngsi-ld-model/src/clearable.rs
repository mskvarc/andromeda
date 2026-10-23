//! A member an update fragment may leave out, delete or set.
//!
//! Clauses 5.4 and 5.5.8 update a Subscription, a Context Source Registration
//! or a Snapshot with a fragment: a member the fragment does not mention keeps
//! its stored value, a member "whose value is an NGSI-LD Null" is deleted, and
//! any other value replaces it. Clause 4.5.0 rules a JSON `null` out ("null
//! cannot be used as a value") and spells the NGSI-LD Null as the URI
//! `urn:ngsi-ld:null`, or as an array holding it alone where an array is
//! required (read by [`clearable_array`](crate::clearable_array)).
//!
//! The URI is read as the Null on a member of any data type. A member typed as
//! a number, a boolean or an object can never hold that string, so the Null is
//! never mistaken for one of its values; on a text member clause 5.5.4 refuses
//! the URI as a first-level member value anywhere but in a fragment, so there
//! too it is never a value. Clause 5.2.1 refuses the Null outside an update,
//! which [`Clearable::into_created`] leaves to the caller to have checked.

use andromeda_ngsi_ld_number::json_value::reader_private_members::SERDE_JSON_PRIVATE_NUMBER_MEMBER;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::{NULL_URI, is_ngsi_ld_null};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{
        DeserializeSeed,
        Error as DeserializeError,
        MapAccess,
        SeqAccess,
        Unexpected,
        Visitor,
        value::{
            BoolDeserializer,
            BorrowedStrDeserializer,
            F64Deserializer,
            I64Deserializer,
            MapAccessDeserializer,
            SeqAccessDeserializer,
            StrDeserializer,
            StringDeserializer,
            U64Deserializer,
        },
    },
};
use serde_json::Deserializer as JsonReader;
use std::{fmt, marker::PhantomData};

/// A member of an update fragment, as the fragment wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clearable<T> {
    /// The fragment does not mention the member, which keeps its stored value.
    Absent,
    /// The fragment wrote the NGSI-LD Null (clause 4.5.0), which deletes the
    /// member from a stored one (clause 5.5.8).
    NgsiLdNull,
    /// The fragment wrote a value, which replaces the stored one.
    Set(T),
}

impl<T> Clearable<T> {
    /// The member a fragment does not mention.
    #[must_use]
    pub const fn absent() -> Clearable<T> {
        Clearable::Absent
    }

    /// Whether the fragment does not mention the member.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, Clearable::Absent)
    }

    /// The member of a body that names a value or leaves the member out, which
    /// is how a stored resource is written back as a request.
    #[must_use]
    pub fn from_option(value: Option<T>) -> Clearable<T> {
        value.map_or(Clearable::Absent, Clearable::Set)
    }

    /// The member a fragment wrote as the string `text`: the NGSI-LD Null, or
    /// the value `parse` reads from it.
    ///
    /// # Errors
    ///
    /// Returns `parse`'s refusal of a string that is not the Null.
    pub fn from_text<E>(text: &str, parse: impl FnOnce(&str) -> Result<T, E>) -> Result<Clearable<T>, E> {
        if is_ngsi_ld_null(text) {
            return Ok(Clearable::NgsiLdNull);
        }
        parse(text).map(Clearable::Set)
    }

    /// Whether the fragment wrote the NGSI-LD Null, which clause 5.2.1 refuses
    /// outside an update.
    #[must_use]
    pub const fn is_ngsi_ld_null(&self) -> bool {
        matches!(self, Clearable::NgsiLdNull)
    }

    /// The value the fragment wrote, if it wrote one.
    #[must_use]
    pub const fn as_set(&self) -> Option<&T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::NgsiLdNull => None,
        }
    }

    /// The value the fragment wrote, if it wrote one, for the caller to change.
    pub const fn as_set_mut(&mut self) -> Option<&mut T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::NgsiLdNull => None,
        }
    }

    /// The member in the same state, its value converted by `convert`.
    #[must_use]
    pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> Clearable<U> {
        match self {
            Clearable::Absent => Clearable::Absent,
            Clearable::NgsiLdNull => Clearable::NgsiLdNull,
            Clearable::Set(value) => Clearable::Set(convert(value)),
        }
    }

    /// The member a create body holds: a create has nothing stored, so a
    /// member it leaves out is absent. A create that wrote the NGSI-LD Null is
    /// refused before this is read (clause 5.2.1).
    #[must_use]
    pub fn into_created(self) -> Option<T> {
        match self {
            Clearable::Set(value) => Some(value),
            Clearable::Absent | Clearable::NgsiLdNull => None,
        }
    }

    /// The member after the fragment is applied over the `stored` one.
    #[must_use]
    pub fn applied_over(self, stored: Option<T>) -> Option<T> {
        match self {
            Clearable::Absent => stored,
            Clearable::NgsiLdNull => None,
            Clearable::Set(value) => Some(value),
        }
    }

    /// Write the member as the fragment wrote it, its value by `set`: the
    /// NGSI-LD Null as the URI, and an absent member, which a carrier field
    /// skips, as nothing.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when the member cannot be written.
    pub fn serialize_with<S: Serializer>(&self, serializer: S, set: impl FnOnce(&T, S) -> Result<S::Ok, S::Error>) -> Result<S::Ok, S::Error> {
        match self {
            Clearable::Set(value) => set(value, serializer),
            Clearable::NgsiLdNull => serializer.serialize_str(NULL_URI.as_str()),
            Clearable::Absent => serializer.serialize_none(),
        }
    }
}

impl<T: Clone> Clearable<T> {
    /// The member after a borrowed fragment is applied over the `stored` one,
    /// as [`Clearable::applied_over`] gives it.
    #[must_use]
    pub fn resolved_over(&self, stored: Option<T>) -> Option<T> {
        match self {
            Clearable::Absent => stored,
            Clearable::NgsiLdNull => None,
            // clone: the fragment is borrowed and the resolved member owns its value.
            Clearable::Set(value) => Some(value.clone()),
        }
    }
}

// A derive would bound `T: Default`, which a member such as a `GeoQuery` is not,
// while the absent member needs no value.
impl<T> Default for Clearable<T> {
    fn default() -> Clearable<T> {
        Clearable::absent()
    }
}

// A derive cannot read the NGSI-LD Null of a member whose own type is no
// string: the string is told from the value before the value is read.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Clearable<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Clearable<T>, D::Error> {
        deserializer.deserialize_any(ClearableVisitor(PhantomData))
    }
}

// A derive cannot write the NGSI-LD Null as the URI and leave `Absent`
// unwritten; a carrier field skips an absent member with `skip_serializing_if`.
impl<T: Serialize> Serialize for Clearable<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize_with(serializer, T::serialize)
    }
}

/// Reads a member: the NGSI-LD Null is told from a value of `T`, which every
/// other JSON value is handed on to be read as, and a JSON `null` is refused.
struct ClearableVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ClearableVisitor<T> {
    type Value = Clearable<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a value or the NGSI-LD Null \"urn:ngsi-ld:null\"")
    }

    fn visit_bool<E: DeserializeError>(self, value: bool) -> Result<Clearable<T>, E> {
        T::deserialize(BoolDeserializer::new(value)).map(Clearable::Set)
    }

    fn visit_i64<E: DeserializeError>(self, value: i64) -> Result<Clearable<T>, E> {
        T::deserialize(I64Deserializer::new(value)).map(Clearable::Set)
    }

    fn visit_u64<E: DeserializeError>(self, value: u64) -> Result<Clearable<T>, E> {
        T::deserialize(U64Deserializer::new(value)).map(Clearable::Set)
    }

    fn visit_f64<E: DeserializeError>(self, value: f64) -> Result<Clearable<T>, E> {
        T::deserialize(F64Deserializer::new(value)).map(Clearable::Set)
    }

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<Clearable<T>, E> {
        Clearable::from_text(text, |text| T::deserialize(StrDeserializer::new(text)))
    }

    fn visit_borrowed_str<E: DeserializeError>(self, text: &'de str) -> Result<Clearable<T>, E> {
        if is_ngsi_ld_null(text) {
            return Ok(Clearable::NgsiLdNull);
        }
        T::deserialize(BorrowedStrDeserializer::new(text)).map(Clearable::Set)
    }

    fn visit_string<E: DeserializeError>(self, text: String) -> Result<Clearable<T>, E> {
        if is_ngsi_ld_null(&text) {
            return Ok(Clearable::NgsiLdNull);
        }
        T::deserialize(StringDeserializer::new(text)).map(Clearable::Set)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, elements: A) -> Result<Clearable<T>, A::Error> {
        T::deserialize(SeqAccessDeserializer::new(elements)).map(Clearable::Set)
    }

    // With `arbitrary_precision`, `serde_json` hands a number that is no 64-bit
    // integer over as a map of one private member holding its text; the text
    // is read back as the number by `serde_json`'s own reader, which a `T` such
    // as `f64` asks for a number directly.
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Clearable<T>, A::Error> {
        let first = members.next_key::<String>()?;
        if first.as_deref() == Some(SERDE_JSON_PRIVATE_NUMBER_MEMBER) {
            let number = members.next_value::<String>()?;
            return T::deserialize(&mut JsonReader::from_reader(number.as_bytes()))
                .map(Clearable::Set)
                .map_err(A::Error::custom);
        }
        T::deserialize(MapAccessDeserializer::new(ReplayedMembers { first, members })).map(Clearable::Set)
    }

    // Clause 4.5.0: "null cannot be used as a value in NGSI-LD".
    fn visit_unit<E: DeserializeError>(self) -> Result<Clearable<T>, E> {
        Err(E::invalid_type(Unexpected::Unit, &self))
    }

    fn visit_none<E: DeserializeError>(self) -> Result<Clearable<T>, E> {
        Err(E::invalid_type(Unexpected::Unit, &self))
    }
}

/// The members of an object whose first name was read before the object was
/// handed on: that name first, then the members still to come.
struct ReplayedMembers<A> {
    /// The name read first, until it is handed on; `None` once it is, and for
    /// an object without members.
    first: Option<String>,
    /// The members after the first.
    members: A,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for ReplayedMembers<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
        match self.first.take() {
            Some(name) => seed.deserialize(StringDeserializer::new(name)).map(Some),
            None => self.members.next_key_seed(seed),
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.members.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.members.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use crate::clearable::Clearable;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::collections::BTreeMap;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Serialize, Deserialize)]
    struct Fragment {
        #[serde(default, skip_serializing_if = "Clearable::is_absent")]
        number: Clearable<f64>,
        #[serde(default, skip_serializing_if = "Clearable::is_absent")]
        flag: Clearable<bool>,
        #[serde(default, skip_serializing_if = "Clearable::is_absent")]
        text: Clearable<String>,
        #[serde(default, skip_serializing_if = "Clearable::is_absent")]
        object: Clearable<BTreeMap<String, u8>>,
    }

    fn read(fragment: &serde_json::Value) -> Result<Fragment, serde_json::Error> {
        serde_json::from_str(&fragment.to_string())
    }

    /// Clause 5.5.8: a member is told absent from deleted from set, and each
    /// is written back as it was read.
    #[test]
    fn a_member_is_told_absent_from_deleted_from_set() {
        let absent = read(&json!({})).expect("absent");
        assert!(absent.number.is_absent() && absent.flag.is_absent() && absent.text.is_absent() && absent.object.is_absent());
        let set = read(&json!({"number": 5, "flag": false, "text": "a", "object": {"k": 1}})).expect("values");
        pretty_assertions::assert_eq!(set.number, Clearable::Set(5.0));
        pretty_assertions::assert_eq!(set.flag, Clearable::Set(false));
        pretty_assertions::assert_eq!(set.text, Clearable::Set("a".to_owned()));
        pretty_assertions::assert_eq!(set.object, Clearable::Set(BTreeMap::from([("k".to_owned(), 1)])));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("writes"), json!({}));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&set).expect("writes"),
            json!({"number": 5.0, "flag": false, "text": "a", "object": {"k": 1}})
        );
    }

    /// Clause 4.5.0 with clause 5.5.8: the NGSI-LD Null deletes a member of any
    /// data type, a number, a boolean, a text and an object alike, and is
    /// written back as the URI.
    #[test]
    fn the_ngsi_ld_null_deletes_a_member_of_any_data_type() {
        let null = json!({"number": "urn:ngsi-ld:null", "flag": "urn:ngsi-ld:null", "text": "urn:ngsi-ld:null", "object": "urn:ngsi-ld:null"});
        let deleted = read(&null).expect("the Null");
        assert!(deleted.number.is_ngsi_ld_null() && deleted.flag.is_ngsi_ld_null() && deleted.text.is_ngsi_ld_null() && deleted.object.is_ngsi_ld_null());
        pretty_assertions::assert_eq!(serde_json::to_value(&deleted).expect("writes"), null);
    }

    /// Clause 4.5.0: "null cannot be used as a value in NGSI-LD".
    #[test]
    fn a_json_null_is_refused_on_every_member() {
        for member in ["number", "flag", "text", "object"] {
            let refusal = read(&json!({ member: null })).expect_err("a JSON null is no value");
            assert!(refusal.to_string().starts_with("invalid type: null"), "{member}: {refusal}");
        }
    }

    #[test]
    fn a_member_of_the_wrong_type_is_refused() {
        for fragment in [
            json!({"number": "five"}),
            json!({"flag": "false"}),
            json!({"flag": 1}),
            json!({"text": 5}),
            json!({"object": [1]}),
        ] {
            assert!(read(&fragment).is_err(), "{fragment}");
        }
    }

    /// A number that is no integer is read whole, and an object member keeps
    /// every member, the first one included.
    #[test]
    fn a_fraction_and_every_member_of_an_object_are_read() {
        let values = read(&json!({"number": 2.5, "object": {"a": 1, "b": 2}})).expect("values");
        pretty_assertions::assert_eq!(values.number, Clearable::Set(2.5));
        pretty_assertions::assert_eq!(values.object, Clearable::Set(BTreeMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)])));
        let empty = read(&json!({"object": {}})).expect("an empty object");
        pretty_assertions::assert_eq!(empty.object, Clearable::Set(BTreeMap::new()));
    }

    /// Only the one spelling clause 3.1 defines is the Null.
    #[test]
    fn a_string_that_only_resembles_the_null_is_a_value() {
        let text = read(&json!({"text": "ngsi-ld:null"})).expect("a value");
        pretty_assertions::assert_eq!(text.text, Clearable::Set("ngsi-ld:null".to_owned()));
    }

    /// Clause 5.8.3: applied over a stored member, an absent one keeps it, a
    /// deleted one removes it and a set one replaces it.
    #[test]
    fn a_fragment_member_is_applied_over_the_stored_one() {
        pretty_assertions::assert_eq!(Clearable::Absent.applied_over(Some(1)), Some(1));
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.applied_over(Some(1)), None);
        pretty_assertions::assert_eq!(Clearable::Set(2).applied_over(Some(1)), Some(2));
        pretty_assertions::assert_eq!(Clearable::Set(2).applied_over(None), Some(2));
    }

    /// A create has nothing stored, so only a set member survives.
    #[test]
    fn a_created_member_is_present_only_when_set() {
        pretty_assertions::assert_eq!(Clearable::<u8>::Absent.into_created(), None);
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.into_created(), None);
        pretty_assertions::assert_eq!(Clearable::Set(3).into_created(), Some(3));
    }

    #[test]
    fn mapping_a_member_keeps_its_state_and_converts_its_value() {
        pretty_assertions::assert_eq!(Clearable::<u8>::Absent.map(u16::from), Clearable::Absent);
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.map(u16::from), Clearable::NgsiLdNull);
        pretty_assertions::assert_eq!(Clearable::Set(3_u8).map(u16::from), Clearable::Set(3_u16));
    }

    #[test]
    fn a_borrowed_fragment_member_resolves_over_the_stored_one() {
        pretty_assertions::assert_eq!(Clearable::<u8>::Absent.resolved_over(Some(1)), Some(1));
        pretty_assertions::assert_eq!(Clearable::<u8>::NgsiLdNull.resolved_over(Some(1)), None);
        pretty_assertions::assert_eq!(Clearable::Set(2).resolved_over(Some(1)), Some(2));
    }

    #[test]
    fn a_text_member_is_parsed_unless_it_is_the_null() {
        pretty_assertions::assert_eq!(Clearable::from_text("7", str::parse::<u8>), Ok(Clearable::Set(7)));
        pretty_assertions::assert_eq!(Clearable::from_text("urn:ngsi-ld:null", str::parse::<u8>), Ok(Clearable::NgsiLdNull));
        assert!(Clearable::from_text("seven", str::parse::<u8>).is_err());
    }
}
