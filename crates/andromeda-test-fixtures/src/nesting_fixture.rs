//! Stored documents nesting one attribute as deeply as asked, and the depth a
//! JSON text nests to.
//!
//! Every reader of a stored or queued document has to decode the deepest
//! attribute a write is admitted with. These build that attribute, in each
//! shape an attribute nests in, as the stored expanded document holds it.

use serde_json::{Value, json};

/// The expanded name of the attribute every fixture nests.
pub const NESTED_ATTRIBUTE: &str = "https://example.org/nested";

/// The expanded name each nested node, and each nested sub-attribute, is a
/// member under.
pub const NESTED_MEMBER: &str = "https://example.org/inner";

/// The shapes an attribute nests in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestingShape {
    /// A Property whose value is a chain of nodes, one per level.
    ObjectChain,
    /// A `ListProperty` whose `valueList` is a chain of lists, one per level.
    ListChain,
    /// A `JsonProperty` whose JSON literal is a chain of objects, one per
    /// level.
    JsonChain,
    /// A chain of sub-attributes, one per level, each a Property of a scalar.
    SubAttributeChain,
}

impl NestingShape {
    /// Every shape.
    pub const ALL: [NestingShape; 4] = [
        NestingShape::ObjectChain,
        NestingShape::ListChain,
        NestingShape::JsonChain,
        NestingShape::SubAttributeChain,
    ];
}

/// The expanded IRI of an NGSI-LD core term.
fn core(term: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/{term}")
}

/// A typed `DateTime` member.
fn date_time(text: &str) -> Value {
    json!([{"@type": core("DateTime"), "@value": text}])
}

/// An instance of `family` carrying `members`, with the system times every
/// stored instance carries.
fn stored_instance(family: &str, members: &[(String, Value)]) -> Value {
    let mut instance = serde_json::Map::new();
    instance.insert("@type".to_owned(), json!([core(family)]));
    instance.extend(members.iter().cloned());
    instance.insert(core("createdAt"), date_time("2026-01-01T00:00:00Z"));
    instance.insert(core("modifiedAt"), date_time("2026-01-01T00:00:00Z"));
    Value::Object(instance)
}

/// A chain of `levels` expanded nodes ending in a string value object.
#[must_use]
pub fn node_chain(levels: usize) -> Value {
    (0..levels).fold(json!({"@value": "leaf"}), |inner, _| json!({NESTED_MEMBER: [inner]}))
}

/// A chain of `levels` list objects, the innermost holding one number.
#[must_use]
pub fn list_chain(levels: usize) -> Value {
    (1..levels).fold(json!({"@list": [{"@value": 1}]}), |inner, _| json!({"@list": [inner]}))
}

/// A chain of `levels` JSON objects ending in a string.
#[must_use]
pub fn object_chain(levels: usize) -> Value {
    (0..levels).fold(json!("leaf"), |inner, _| json!({"k": inner}))
}

/// The stored attribute instance nesting `levels` levels in `shape`.
#[must_use]
pub fn nested_instance(shape: NestingShape, levels: usize) -> Value {
    match shape {
        NestingShape::ObjectChain => stored_instance("Property", &[(core("hasValue"), json!([node_chain(levels)]))]),
        NestingShape::ListChain => stored_instance("ListProperty", &[(core("hasValueList"), json!([list_chain(levels)]))]),
        NestingShape::JsonChain => stored_instance(
            "JsonProperty",
            &[(core("hasJSON"), json!([{"@type": "@json", "@value": object_chain(levels)}]))],
        ),
        NestingShape::SubAttributeChain => (0..levels).fold(stored_instance("Property", &[(core("hasValue"), json!([{"@value": 1}]))]), |inner, _| {
            stored_instance(
                "Property",
                &[(core("hasValue"), json!([{"@value": 1}])), (NESTED_MEMBER.to_owned(), json!([inner]))],
            )
        }),
    }
}

/// The stored entity document of `id` holding `instance` under
/// [`NESTED_ATTRIBUTE`].
#[must_use]
pub fn stored_entity(id: &str, instance: Value) -> Value {
    let node = serde_json::Map::from_iter([
        ("@id".to_owned(), json!(id)),
        ("@type".to_owned(), json!(["https://example.org/NestingProbe"])),
        (NESTED_ATTRIBUTE.to_owned(), Value::Array(vec![instance])),
    ]);
    Value::Array(vec![Value::Object(node)])
}

/// How many arrays and objects the deepest value of a JSON text sits in,
/// counting the containers themselves.
#[must_use]
pub fn container_depth(text: &[u8]) -> usize {
    let mut depth = 0_usize;
    let mut deepest = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text {
        match (in_string, escaped, byte) {
            (true, true, _) => escaped = false,
            (true, false, b'\\') => escaped = true,
            (true, false, b'"') | (false, _, b'"') => in_string = !in_string,
            (false, _, b'[' | b'{') => {
                depth = depth.saturating_add(1);
                deepest = deepest.max(depth);
            }
            (false, _, b']' | b'}') => depth = depth.saturating_sub(1),
            (true, false, _) | (false, _, _) => {}
        }
    }
    deepest
}

#[cfg(test)]
mod tests {
    use crate::nesting_fixture::{NestingShape, container_depth, nested_instance, stored_entity};

    #[test]
    fn the_depth_of_a_text_counts_its_containers_and_skips_strings() {
        pretty_assertions::assert_eq!(container_depth(b"1"), 0);
        pretty_assertions::assert_eq!(container_depth(b"[]"), 1);
        pretty_assertions::assert_eq!(container_depth(br#"{"a":[{"b":"[[[{"}]}"#), 3);
        pretty_assertions::assert_eq!(container_depth(br#"["\"[", [[]]]"#), 3);
    }

    /// Each level costs two containers of the stored entity document, the
    /// JSON literal's one; the document and the leaf take six.
    #[test]
    fn a_stored_entity_nests_two_containers_a_level_and_six_around() {
        for shape in NestingShape::ALL {
            for levels in [1, 7, 59] {
                let text = stored_entity("urn:ngsi-ld:NestingProbe:1", nested_instance(shape, levels)).to_string();
                let expected = match shape {
                    NestingShape::JsonChain => levels + 6,
                    NestingShape::ObjectChain | NestingShape::ListChain | NestingShape::SubAttributeChain => 2 * levels + 6,
                };
                pretty_assertions::assert_eq!(container_depth(text.as_bytes()), expected, "{shape:?} at {levels}");
            }
        }
    }
}
