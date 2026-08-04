//! The single-or-array shape NGSI-LD uses for `String` / `String[]` members.

use crate::{
    attribute::{expansion::id_object, reference::NgsiLdReference},
    uri::Uri,
};
use compact_str::CompactString;
use jsonld::Expandable;
use jsonld_expandable_core::{JsonValue, ToJsonValue};
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
}

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

impl Expandable for OneOrMany<Uri> {
    fn expand<V: JsonValue>(&self) -> V {
        V::array(self.iter().map(|value| id_object::<V>(value.as_ref())))
    }
}

impl<V: JsonValue> ToJsonValue<V> for OneOrMany<Uri> {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}

impl Expandable for OneOrMany<NgsiLdReference> {
    fn expand<V: JsonValue>(&self) -> V {
        V::array(self.iter().map(|value| id_object::<V>(value.as_ref())))
    }
}

impl<V: JsonValue> ToJsonValue<V> for OneOrMany<NgsiLdReference> {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}

impl Expandable for OneOrMany<CompactString> {
    fn expand<V: JsonValue>(&self) -> V {
        V::array(self.iter().map(|value| id_object::<V>(value.as_str())))
    }
}

impl<V: JsonValue> ToJsonValue<V> for OneOrMany<CompactString> {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::attribute::one_or_many::OneOrMany;

    #[test]
    fn one_or_many_iterates_single_and_many() {
        let single = OneOrMany::One(1_u8);
        let many = OneOrMany::Many(vec![1_u8, 2_u8]);
        pretty_assertions::assert_eq!(single.iter().count(), 1);
        pretty_assertions::assert_eq!(many.iter().count(), 2);
    }
}
