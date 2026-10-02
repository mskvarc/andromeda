//! Adding a compacted value under a key, as the JSON-LD 1.1 API "add value"
//! algorithm (section 9.4) does.
//!
//! A key that already holds a value becomes an array of both, an array value
//! is added element by element, and a key whose term keeps its values in an
//! array starts as one. The library applies the same rule inside its own
//! compaction; it is reproduced here because the walk writes its objects
//! itself.

use crate::compacted::{Compacted, CompactedObject};
use std::mem;

/// Add `value` under `key` in `object`, as an array when `as_array` holds.
pub(crate) fn add_value<'a>(object: &mut CompactedObject<'a>, key: &str, value: Compacted<'a>, as_array: bool) {
    match value {
        Compacted::Array(values) => {
            // The entry's shape is settled before the elements are added: an
            // existing single value becomes the first element of an array, and
            // an absent entry of an array-valued term starts as an empty array
            // so that an empty `values` still leaves one behind.
            match object.get_mut(key) {
                Some(existing) if !existing.is_array() => {
                    let previous = mem::replace(existing, Compacted::Array(Vec::new()));
                    if let Compacted::Array(items) = existing {
                        items.push(previous);
                    }
                }
                None if as_array => {
                    object.insert(key, Compacted::Array(Vec::new()));
                }
                Some(_) | None => {}
            }
            for value in values {
                add_value(object, key, value, false);
            }
        }
        scalar @ (Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) | Compacted::Object(_) | Compacted::Geometry(_)) => {
            match object.get_mut(key) {
                Some(Compacted::Array(items)) => items.push(scalar),
                Some(existing) => {
                    let previous = mem::replace(existing, Compacted::Array(Vec::new()));
                    if let Compacted::Array(items) = existing {
                        items.push(previous);
                        items.push(scalar);
                    }
                }
                None if as_array => {
                    object.insert(key, Compacted::Array(vec![scalar]));
                }
                None => {
                    object.insert(key, scalar);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compacted::{Compacted, CompactedObject},
        member_insertion::add_value,
    };

    fn text(value: &str) -> Compacted<'static> {
        Compacted::String(Box::from(value))
    }

    fn rendered(object: CompactedObject<'_>) -> serde_json::Value {
        serde_json::Value::Object(object.into_written_json().expect("no geometry"))
    }

    #[test]
    fn a_first_value_is_written_bare_unless_the_term_keeps_an_array() {
        let mut bare = CompactedObject::new();
        add_value(&mut bare, "a", text("x"), false);
        let mut kept = CompactedObject::new();
        add_value(&mut kept, "a", text("x"), true);

        pretty_assertions::assert_eq!(rendered(bare), serde_json::json!({"a": "x"}));
        pretty_assertions::assert_eq!(rendered(kept), serde_json::json!({"a": ["x"]}));
    }

    #[test]
    fn a_second_value_turns_the_entry_into_an_array_of_both() {
        let mut object = CompactedObject::new();
        add_value(&mut object, "a", text("x"), false);
        add_value(&mut object, "a", text("y"), false);
        add_value(&mut object, "a", text("z"), false);

        pretty_assertions::assert_eq!(rendered(object), serde_json::json!({"a": ["x", "y", "z"]}));
    }

    #[test]
    fn an_array_value_is_added_element_by_element() {
        let mut object = CompactedObject::new();
        add_value(&mut object, "a", text("x"), false);
        add_value(&mut object, "a", Compacted::Array(vec![text("y"), text("z")]), false);

        pretty_assertions::assert_eq!(rendered(object), serde_json::json!({"a": ["x", "y", "z"]}));
    }

    #[test]
    fn an_empty_array_leaves_an_empty_array_only_for_an_array_valued_term() {
        let mut kept = CompactedObject::new();
        add_value(&mut kept, "a", Compacted::Array(Vec::new()), true);
        let mut bare = CompactedObject::new();
        add_value(&mut bare, "a", Compacted::Array(Vec::new()), false);

        pretty_assertions::assert_eq!(rendered(kept), serde_json::json!({"a": []}));
        pretty_assertions::assert_eq!(rendered(bare), serde_json::json!({}));
    }
}
