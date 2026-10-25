//! The member order of the compacted documents the engine answers.
//!
//! RFC 8259 section 4 gives the members of a JSON object no order, so the order
//! a compacted document is written in is the broker's to choose. The engine
//! writes every object with its members in key order, at every level: the order the term walk reproduces, so an entity reads the same whether the walk or the
//! processor compacted it.

use jstrict::Value;

/// Put the members of every object in `value` in key order.
///
/// The tree is walked with a worklist rather than by recursion, so a document
/// of any depth is ordered on any stack.
pub(crate) fn sort_members(value: &mut Value) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(items) => pending.extend(items.iter_mut()),
            Value::Object(object) => {
                object.sort();
                pending.extend(object.iter_mut().map(|(_, member)| member));
            }
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::engine::member_order::sort_members;
    use jstrict::{Print, parse::parse_str_value};
    use std::thread;

    #[test]
    fn every_object_is_written_in_key_order_and_arrays_keep_theirs() {
        let mut value = parse_str_value(r#"{"b":[{"z":1,"a":2},3,1],"a":{"y":null,"x":true}}"#).expect("JSON");

        sort_members(&mut value);

        pretty_assertions::assert_eq!(value.compact_print().to_string(), r#"{"a":{"x":true,"y":null},"b":[{"a":2,"z":1},3,1]}"#);
    }

    #[test]
    fn a_deep_document_is_ordered_without_recursing() {
        const LEVELS: usize = 10_000;
        let text = format!("{}{{}}{}", r#"{"b":0,"a":"#.repeat(LEVELS), "}".repeat(LEVELS));
        let mut value = parse_str_value(&text).expect("JSON");

        let ordered = thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                sort_members(&mut value);
                value
            })
            .expect("the thread starts")
            .join()
            .expect("the ordering does not overflow its stack");

        let printed = ordered.compact_print().to_string();
        assert!(printed.starts_with(r#"{"a":{"a":"#), "{}", &printed[..32]);
    }
}
