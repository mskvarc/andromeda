//! The single-or-array shape NGSI-LD uses for `String` / `String[]` members.
//!
//! The member is read by the kind of JSON it is: a string is one element, an
//! array is the elements, and anything else is refused as neither. Each
//! element's own refusal (a name outside its grammar, a tag that is no
//! language tag) is the refusal of the member.

use crate::attribute::recorded_elements::recorded_elements;
use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{DeserializeSeed, Error as DeserializeError, Expected, SeqAccess, Visitor, value::StrDeserializer},
};
use std::{
    fmt,
    marker::PhantomData,
    slice::{Iter, from_ref},
};

/// A value that can appear in `String` or `String[]` NGSI-LD fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    /// A single value.
    One(T),
    /// Multiple values.
    Many(Vec<T>),
}

// `#[derive(Default)]` needs a `#[default]` variant and `#[default]` only
// applies to unit variants, so neither `One(T)` nor `Many(Vec<T>)` can carry
// it. One hand-written impl here replaces the ones that would otherwise sit on
// every attribute struct holding a `OneOrMany` field.
impl<T> Default for OneOrMany<T> {
    fn default() -> OneOrMany<T> {
        OneOrMany::Many(Vec::new())
    }
}

impl<T> OneOrMany<T> {
    /// Return an iterator over the contained values.
    pub fn iter(&self) -> Iter<'_, T> {
        match self {
            OneOrMany::One(value) => from_ref(value).iter(),
            OneOrMany::Many(values) => values.iter(),
        }
    }

    /// Give the contained values up, in order.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        match self {
            OneOrMany::One(value) => vec![value],
            OneOrMany::Many(values) => values,
        }
    }
}

impl<'de, T: Deserialize<'de>> OneOrMany<T> {
    /// Read a string or an array of elements, refusing any other JSON kind
    /// with `expected` as what was asked for.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's refusal of a member that is neither a string
    /// nor an array, and the element's own refusal otherwise.
    pub fn deserialize_expecting<D: Deserializer<'de>, X: Expected>(deserializer: D, expected: X) -> Result<OneOrMany<T>, D::Error> {
        OneOrMany::deserialize_expecting_within(deserializer, expected, None)
    }

    /// Read a string or an array of elements as
    /// [`OneOrMany::deserialize_expecting`] does, the elements recorded against
    /// `budget` when there is one.
    ///
    /// # Errors
    ///
    /// As [`OneOrMany::deserialize_expecting`].
    pub fn deserialize_expecting_within<D: Deserializer<'de>, X: Expected>(
        deserializer: D,
        expected: X,
        budget: Option<&ValueBudget>,
    ) -> Result<OneOrMany<T>, D::Error> {
        deserializer.deserialize_any(OneOrManyVisitor {
            expected,
            budget,
            element: PhantomData,
        })
    }
}

impl<T> OneOrMany<T> {
    /// Read a member as a string or an array of strings, its elements recorded
    /// against `budget` when there is one.
    #[must_use]
    pub const fn within(budget: Option<&ValueBudget>) -> OneOrManyWithin<'_, T> {
        OneOrManyWithin { budget, element: PhantomData }
    }
}

/// Reads a `String` or `String[]` member, its elements recorded against the
/// budget of values the entity it belongs to may still hold.
#[derive(Debug)]
pub struct OneOrManyWithin<'b, T> {
    /// What the elements are recorded against, when the entity is held to a
    /// limit.
    budget: Option<&'b ValueBudget>,
    /// The element type.
    element: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for OneOrManyWithin<'_, T> {
    type Value = OneOrMany<T>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<OneOrMany<T>, D::Error> {
        OneOrMany::deserialize_expecting_within(deserializer, StringOrStrings, self.budget)
    }
}

// Hand-written: a derived untagged union discards each variant's error and
// reports only that none matched, so an element's refusal would never reach
// the member's. The visitor dispatches on the JSON kind instead.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<OneOrMany<T>, D::Error> {
        OneOrMany::deserialize_expecting(deserializer, StringOrStrings)
    }
}

/// What a `String` or `String[]` member asks for.
struct StringOrStrings;

// `Expected` has no derive: it is the text a kind refusal names.
impl Expected for StringOrStrings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string or an array of strings")
    }
}

/// Reads one element from a string and the elements from an array.
struct OneOrManyVisitor<'b, T, X> {
    /// What a refusal of any other JSON kind says was expected.
    expected: X,
    /// What the elements are recorded against, when the entity is held to a
    /// limit.
    budget: Option<&'b ValueBudget>,
    /// The element type.
    element: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>, X: Expected> Visitor<'de> for OneOrManyVisitor<'_, T, X> {
    type Value = OneOrMany<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.expected.fmt(formatter)
    }

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<OneOrMany<T>, E> {
        if let Some(budget) = self.budget {
            budget.spend();
        }
        T::deserialize(StrDeserializer::<E>::new(text)).map(OneOrMany::One)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, elements: A) -> Result<OneOrMany<T>, A::Error> {
        recorded_elements(elements, self.budget).map(OneOrMany::Many)
    }
}

// Borrowed iteration goes through the inherent `iter`; the impl is what lets a
// `&OneOrMany<T>` be used directly in a `for` loop or an iterator adapter.
impl<'a, T> IntoIterator for &'a OneOrMany<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

// `derive_more::Display` renders each variant with one format string; the
// `Many` arm has to join its elements, which a format string cannot express.
impl<T> fmt::Display for OneOrMany<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OneOrMany::One(value) => write!(f, "{value}"),
            OneOrMany::Many(values) => {
                let joined = values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
                f.write_str(&joined)
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::attribute::one_or_many::OneOrMany;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;

    #[test]
    fn one_or_many_iterates_single_and_many() {
        let single = OneOrMany::One(1_u8);
        let many = OneOrMany::Many(vec![1_u8, 2_u8]);
        pretty_assertions::assert_eq!(single.iter().count(), 1);
        pretty_assertions::assert_eq!(many.iter().count(), 2);
    }

    #[test]
    fn a_string_reads_as_one_element_and_an_array_as_the_elements() {
        pretty_assertions::assert_eq!(
            serde_json::from_str::<OneOrMany<String>>(r#""a""#).expect("a string"),
            OneOrMany::One("a".to_owned())
        );
        pretty_assertions::assert_eq!(
            serde_json::from_str::<OneOrMany<String>>(r#"["a", "b"]"#).expect("an array"),
            OneOrMany::Many(vec!["a".to_owned(), "b".to_owned()])
        );
    }

    #[test]
    fn an_element_refusal_is_the_refusal_of_the_member_in_both_forms() {
        for written in [r#""Bio-geographicalRegion""#, r#"["Building", "Bio-geographicalRegion"]"#] {
            let refused = serde_json::from_str::<OneOrMany<EntityTypeName>>(written).expect_err(written);
            assert!(
                refused.to_string().starts_with("a name must be a term, a compact IRI or an absolute IRI"),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_member_of_another_kind_is_refused_as_neither_a_string_nor_an_array() {
        for written in ["42", "true", "null", r#"{"a": "b"}"#] {
            let refused = serde_json::from_str::<OneOrMany<String>>(written).expect_err(written);
            assert!(refused.to_string().contains("expected a string or an array of strings"), "{refused}");
        }
    }
}
