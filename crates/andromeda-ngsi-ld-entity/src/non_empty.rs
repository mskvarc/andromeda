//! An ordered collection that holds at least one element.
//!
//! Several members of an NGSI-LD entity are meaningless when empty: a
//! Relationship without an object (clause 4.5.3), a `VocabProperty` without a
//! vocabulary term (clause 4.5.20), an entity without a type (clause 4.5.1).
//! Holding them in this type moves that rule from every reader into the one
//! constructor.

use derive_more::{AsRef, Deref, IntoIterator};
use thiserror::Error;

/// An attempt to build a [`NonEmpty`] from no elements.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the collection holds no element, and at least one is required")]
pub struct EmptyCollection;

/// One or more elements, in order.
#[derive(AsRef, Clone, Debug, Deref, Eq, Hash, IntoIterator, PartialEq)]
#[as_ref(forward)]
#[into_iterator(owned, ref)]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    /// Hold exactly one element.
    #[must_use]
    pub fn one(element: T) -> NonEmpty<T> {
        NonEmpty(vec![element])
    }

    /// Hold the given elements, in order.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyCollection`] when `elements` is empty.
    pub fn try_from_vec(elements: Vec<T>) -> Result<NonEmpty<T>, EmptyCollection> {
        if elements.is_empty() { Err(EmptyCollection) } else { Ok(NonEmpty(elements)) }
    }

    /// The first element, which always exists.
    #[must_use]
    pub fn head(&self) -> &T {
        // Every constructor refuses an empty vector and nothing removes from
        // it, so index zero is always occupied.
        &self.0[0]
    }

    /// Append one element.
    pub fn push(&mut self, element: T) {
        self.0.push(element);
    }

    /// The elements as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// The elements as a mutable slice: each can be changed in place, none
    /// removed, so the collection stays non-empty.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.0
    }

    /// Give the elements up as a vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Map every element, keeping the order and the non-emptiness.
    pub fn map<U>(self, convert: impl FnMut(T) -> U) -> NonEmpty<U> {
        NonEmpty(self.0.into_iter().map(convert).collect())
    }

    /// Map every borrowed element, keeping the order and the non-emptiness.
    pub fn map_ref<U>(&self, convert: impl FnMut(&T) -> U) -> NonEmpty<U> {
        NonEmpty(self.0.iter().map(convert).collect())
    }

    /// Map every element through a fallible conversion, stopping at the first
    /// failure.
    ///
    /// # Errors
    ///
    /// Returns the first error `convert` reports.
    pub fn try_map<U, E>(self, convert: impl FnMut(T) -> Result<U, E>) -> Result<NonEmpty<U>, E> {
        self.0.into_iter().map(convert).collect::<Result<Vec<U>, E>>().map(NonEmpty)
    }
}

impl<T> TryFrom<Vec<T>> for NonEmpty<T> {
    type Error = EmptyCollection;

    fn try_from(elements: Vec<T>) -> Result<NonEmpty<T>, EmptyCollection> {
        NonEmpty::try_from_vec(elements)
    }
}

#[cfg(test)]
mod tests {
    use crate::non_empty::{EmptyCollection, NonEmpty};

    #[test]
    fn an_empty_vector_is_refused() {
        pretty_assertions::assert_eq!(NonEmpty::<u8>::try_from_vec(Vec::new()), Err(EmptyCollection));
    }

    #[test]
    fn the_head_is_the_first_element_in_order() {
        let elements = NonEmpty::try_from_vec(vec![3, 1, 2]).expect("three elements");
        pretty_assertions::assert_eq!(*elements.head(), 3);
        pretty_assertions::assert_eq!(elements.as_slice(), &[3, 1, 2]);
    }

    #[test]
    fn mapping_keeps_order() {
        let mapped = NonEmpty::one(2).map(|value| value * 10);
        pretty_assertions::assert_eq!(mapped.into_vec(), vec![20]);
    }
}
