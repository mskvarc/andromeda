//! Stored entity documents nesting one attribute as deeply as a test asks, in
//! each shape an attribute nests in (clause 4.5), as the expanded document
//! holds them.

use serde_json::{Map, Value, json};

/// The expanded name of the attribute every probe nests.
pub(crate) const NESTED_ATTRIBUTE: &str = "https://example.org/nested";

/// The expanded name each nested node, and each nested sub-attribute, is a
/// member under.
const NESTED_MEMBER: &str = "https://example.org/inner";

/// The chains an attribute nests in, one link per level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NestingChain {
    /// A Property whose value is a chain of nodes.
    Nodes,
    /// A `ListProperty` whose `valueList` is a chain of lists.
    Lists,
    /// A `JsonProperty` whose JSON literal is a chain of objects.
    JsonObjects,
    /// A chain of sub-attributes, each a Property of a scalar.
    SubAttributes,
}

impl NestingChain {
    /// Every chain.
    pub(crate) const ALL: [NestingChain; 4] = [NestingChain::Nodes, NestingChain::Lists, NestingChain::JsonObjects, NestingChain::SubAttributes];
}

/// The expanded IRI of an NGSI-LD core term.
fn core(term: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/{term}")
}

/// An instance of `family` carrying `members`, with the system times every
/// stored instance carries.
fn stored_instance(family: &str, members: Vec<(String, Value)>) -> Value {
    let created = json!([{"@type": core("DateTime"), "@value": "2026-01-01T00:00:00Z"}]);
    let mut instance = Map::from_iter([("@type".to_owned(), json!([core(family)]))]);
    instance.extend(members);
    instance.insert(core("modifiedAt"), created.clone()); // clone: both system times hold the same instant
    instance.insert(core("createdAt"), created);
    Value::Object(instance)
}

/// A chain of `levels` expanded nodes ending in a string value object.
pub(crate) fn node_chain(levels: usize) -> Value {
    (0..levels).fold(json!({"@value": "leaf"}), |inner, _| json!({NESTED_MEMBER: [inner]}))
}

/// The stored attribute instance nesting `levels` levels in `chain`.
pub(crate) fn nested_instance(chain: NestingChain, levels: usize) -> Value {
    match chain {
        NestingChain::Nodes => stored_instance("Property", vec![(core("hasValue"), json!([node_chain(levels)]))]),
        NestingChain::Lists => {
            let lists = (1..levels).fold(json!({"@list": [{"@value": 1}]}), |inner, _| json!({"@list": [inner]}));
            stored_instance("ListProperty", vec![(core("hasValueList"), json!([lists]))])
        }
        NestingChain::JsonObjects => {
            let objects = (0..levels).fold(json!("leaf"), |inner, _| json!({"k": inner}));
            stored_instance("JsonProperty", vec![(core("hasJSON"), json!([{"@type": "@json", "@value": objects}]))])
        }
        NestingChain::SubAttributes => (0..levels).fold(stored_instance("Property", vec![(core("hasValue"), json!([{"@value": 1}]))]), |inner, _| {
            stored_instance(
                "Property",
                vec![(core("hasValue"), json!([{"@value": 1}])), (NESTED_MEMBER.to_owned(), json!([inner]))],
            )
        }),
    }
}

/// The stored entity document of `id` holding `instance` under
/// [`NESTED_ATTRIBUTE`].
pub(crate) fn stored_entity(id: &str, instance: &Value) -> Value {
    json!([{
        "@id": id,
        "@type": ["https://example.org/NestingProbe"],
        NESTED_ATTRIBUTE: [instance],
    }])
}
