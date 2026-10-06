//! The single-or-array shape NGSI-LD uses for `String` / `String[]` members.

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    slice::{Iter, from_ref},
};

/// A value that can appear in `String` or `String[]` NGSI-LD fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
mod tests {
    use crate::attribute::one_or_many::OneOrMany;

    #[test]
    fn one_or_many_iterates_single_and_many() {
        let single = OneOrMany::One(1_u8);
        let many = OneOrMany::Many(vec![1_u8, 2_u8]);
        pretty_assertions::assert_eq!(single.iter().count(), 1);
        pretty_assertions::assert_eq!(many.iter().count(), 2);
    }
}
