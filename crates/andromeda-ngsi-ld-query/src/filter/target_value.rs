//! The clause 4.9 target value an attribute path addresses inside an expanded
//! JSON-LD entity.
//!
//! Clause 4.9 defines a target value for every attribute family of clause
//! 5.2.4, and the definitions differ in shape, not only in which member carries
//! them:
//!
//! * "If the target element is a Property, the target value is defined as the
//!   Value associated to such Property" — one value, or, when the client wrote
//!   an array, the array JSON-LD expansion turned into several `hasValue`
//!   nodes;
//! * "If the target element is a `LanguageProperty`, and no target language is
//!   specified, the target value is defined as a value from any of the
//!   key-value pairs held within the languageMap";
//! * "If the target element is a `ListProperty`, the target value is defined as
//!   the valueList array associated to such a `ListProperty`";
//! * "If the target element is a `ListRelationship`, the target object is
//!   defined as the array of objects associated (represented as URIs) to such
//!   `ListRelationship`";
//! * "If the target element is a `VocabProperty`, the target value shall be
//!   expanded according to the @context";
//! * "If the target element is a Relationship, the target object is defined as
//!   the object associated (represented as a URI or array of URIs)".
//!
//! Every one of those that can hold more than one value resolves to a JSON
//! array, which is what makes the clause's array rules — "Is included in the
//! target value, if the latter is an array", and under `unequal` "Is not
//! included in the target value, if the latter is an array" — apply to it. A
//! resolution that stopped at the first value node instead answered `==` for
//! the first element only and answered `!=` with the exact opposite of what the
//! clause requires, because a target the operand *is* in still had a first
//! element that differed from it.
//!
//! The two list families are the ones whose expanded shape is not a plain array
//! of value nodes: `valueList` and `objectList` are ordered, so JSON-LD
//! expansion wraps them in a single `@list` node, and a lookup for `@value` one
//! level too high finds nothing at all.

use andromeda_ngsi_ld_vocabulary::expanded::properties::{JSON, LANGUAGE_MAP, OBJECT, OBJECT_LIST, VALUE, VALUE_LIST, VOCAB};
use serde_json::Value;

/// The JSON-LD keyword a value node carries a literal under.
pub const VALUE_KEYWORD: &str = "@value";

/// The JSON-LD keyword a node carries an identifier under.
const ID_KEYWORD: &str = "@id";

/// The JSON-LD keyword an ordered member carries its elements under.
const LIST_KEYWORD: &str = "@list";

/// The key a canonical-keyed Context Source property carries its value under.
///
/// Clause 5.2.9 registration properties are not expanded JSON-LD: a normalized
/// one is `{"type": "Property", "value": …}`, written in the compact form the
/// registrant supplied.
const COMPACT_VALUE_KEY: &str = "value";

/// One resolved comparison target: the clause 4.9 target value a predicate
/// compares, and where it came from.
///
/// Clause 4.9 forbids the ordering and pattern operators against a
/// Relationship, so the evaluator has to know a value's origin, not only its
/// JSON shape — a Relationship object and a string Property value are both JSON
/// strings.
pub struct ResolvedTarget {
    /// The target value to compare against the Query Term operand.
    pub value: Value,
    /// Which clause 4.9 definition produced it.
    pub origin: TargetOrigin,
    /// What clause 4.9 makes of it when it holds several values.
    pub reading: MultiValueReading,
}

/// What clause 4.9 makes of an attribute that carries several values.
///
/// The two readings agree on the equality operators and part company on every
/// other one, so which of them applies is a property of the target rather than
/// of the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiValueReading {
    /// The array of values is itself the target value.
    ///
    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`" and a `ListRelationship`'s as
    /// "the array of objects", and a Property's is the Value the client wrote —
    /// an array when they wrote one. The equality operators test membership in
    /// it ("Is included in the target value, if the latter is an array"), while
    /// an operator defined over a scalar or a String target does not match it at
    /// all: "If the target value data type is different than String then it
    /// shall be considered as not matching."
    WholeArray,
    /// Any one of the values is the target value in its own right.
    ///
    /// Clause 4.9: "If the target element is a `LanguageProperty`, and no target
    /// language is specified, the target value is defined as a value from any of
    /// the key-value pairs held within the languageMap." Every operator is
    /// therefore answered by the values themselves, and the negated ones hold
    /// only when none of them satisfies the Query Term — "No matching value is
    /// found in any of the values of the key-value pairs of a languageMap".
    AnyOneValue,
}

/// Which clause 4.9 definition a resolved target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOrigin {
    /// A target *value*: what a Property-family attribute carries.
    AttributeValue,
    /// A target *object*: the URI, or array of URIs, a Relationship or a
    /// `ListRelationship` names.
    RelationshipObject,
}

/// One expanded member a clause 4.9 target value can be carried under.
struct ValueMember {
    /// The expanded IRI of the member (clause 5.2.4).
    member: &'static str,
    /// How the member's array wraps the value nodes.
    carrier: ValueCarrier,
    /// Which JSON-LD keyword each of those nodes carries its value under.
    terminal: TerminalKeyword,
    /// Which clause 4.9 definition the member's contents satisfy.
    origin: TargetOrigin,
    /// What clause 4.9 makes of the member when it carries several values.
    reading: MultiValueReading,
}

/// How an expanded member wraps the nodes holding its values.
enum ValueCarrier {
    /// The member's array holds the value nodes itself, one per value the
    /// client wrote. A single node is one value; several are an array target.
    Nodes,
    /// The member holds one node whose `@list` is the ordered target value.
    ///
    /// Clause 4.9 defines the target of both list families as the array itself,
    /// so it stays an array even when it holds a single element — which is what
    /// keeps the ordering and pattern operators off it, as an array target
    /// matches neither.
    OrderedList,
}

/// The JSON-LD keyword one family's value nodes carry their value under.
enum TerminalKeyword {
    /// A literal, carried under `@value`.
    Literal,
    /// An identifier, carried under `@id` — or under `@value` when the context
    /// the client wrote declared no `@type: "@id"` coercion for the member,
    /// which the core context does not for `objectList`.
    Identifier,
}

impl ValueMember {
    /// The target value this member holds inside one attribute instance, or
    /// `None` when the instance does not carry it in a readable shape.
    ///
    /// A member present but holding no readable value node is not a target: a
    /// structured Property value expands to IRI-keyed member nodes under
    /// `hasValue` that carry no `@value` of their own, and a path addressing one
    /// of those members descends into them instead.
    fn target_in(&self, instance: &Value) -> Option<ResolvedTarget> {
        let nodes = instance.get(self.member)?.as_array()?;
        match self.carrier {
            ValueCarrier::Nodes => match self.terminals(nodes).as_slice() {
                [] => None,
                [single] => Some(ResolvedTarget {
                    value: single.clone(), // clone: the caller owns the target beyond the instance borrow
                    origin: self.origin,
                    reading: self.reading,
                }),
                several => Some(ResolvedTarget {
                    value: Value::Array(several.to_vec()), // clone: the caller owns the target beyond the instance borrow
                    origin: self.origin,
                    reading: self.reading,
                }),
            },
            ValueCarrier::OrderedList => {
                let list = nodes.first()?.get(LIST_KEYWORD)?.as_array()?;
                Some(ResolvedTarget {
                    value: Value::Array(self.terminals(list)),
                    origin: self.origin,
                    reading: self.reading,
                })
            }
        }
    }

    /// The values the given nodes carry, in order, skipping any node that
    /// carries none.
    ///
    /// A node with no terminal keyword holds nothing a Query Term operand could
    /// be identical to or included in, so leaving it out changes no comparison.
    fn terminals(&self, nodes: &[Value]) -> Vec<Value> {
        nodes
            .iter()
            .filter_map(|node| self.terminal_of(node))
            .cloned() // clone: the caller owns the target beyond the instance borrow
            .collect()
    }

    /// The value one node carries under this family's terminal keyword.
    ///
    /// Clause 4.5.22.2 makes a normalized `objectList` "an ordered array of
    /// Relationship Objects each consisting of a JSON object containing a
    /// single Attribute with a key called `object`", so a member of that list
    /// expands to a node carrying `hasObject` rather than to a bare terminal.
    /// Clause 4.5.22.3 admits the concise spelling, a plain URI, which expands
    /// to the terminal directly. Both name the same target object, so the
    /// wrapper is unwrapped rather than treated as a value of its own -- a
    /// reader that stopped at the outer node resolved no target at all, and an
    /// unresolved target satisfies no operator.
    fn terminal_of<'a>(&self, node: &'a Value) -> Option<&'a Value> {
        let node = self.unwrap_relationship_object(node);
        match self.terminal {
            TerminalKeyword::Literal => node.get(VALUE_KEYWORD),
            TerminalKeyword::Identifier => node.get(ID_KEYWORD).or_else(|| node.get(VALUE_KEYWORD)),
        }
    }

    /// The value node inside a normalized Relationship Object wrapper, or the
    /// node itself when it carries no wrapper.
    fn unwrap_relationship_object<'a>(&self, node: &'a Value) -> &'a Value {
        match self.terminal {
            TerminalKeyword::Literal => node,
            TerminalKeyword::Identifier => node.get(OBJECT.as_str()).and_then(|wrapped| wrapped.as_array()?.first()).unwrap_or(node),
        }
    }
}

/// The expanded members a `q` target can terminate at, in the order an instance
/// is searched for one.
///
/// Covers all eight attribute families of clause 5.2.4, not only Property and
/// Relationship, so a `q` over a `LanguageProperty`, `VocabProperty`,
/// `JsonProperty` or a list attribute resolves rather than silently missing.
fn value_members() -> [ValueMember; 7] {
    [
        ValueMember {
            member: VALUE.as_str(),
            carrier: ValueCarrier::Nodes,
            terminal: TerminalKeyword::Literal,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        },
        ValueMember {
            member: OBJECT.as_str(),
            carrier: ValueCarrier::Nodes,
            terminal: TerminalKeyword::Identifier,
            origin: TargetOrigin::RelationshipObject,
            reading: MultiValueReading::WholeArray,
        },
        ValueMember {
            member: VOCAB.as_str(),
            carrier: ValueCarrier::Nodes,
            terminal: TerminalKeyword::Identifier,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        },
        ValueMember {
            member: LANGUAGE_MAP.as_str(),
            carrier: ValueCarrier::Nodes,
            terminal: TerminalKeyword::Literal,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::AnyOneValue,
        },
        ValueMember {
            member: JSON.as_str(),
            carrier: ValueCarrier::Nodes,
            terminal: TerminalKeyword::Literal,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        },
        ValueMember {
            member: VALUE_LIST.as_str(),
            carrier: ValueCarrier::OrderedList,
            terminal: TerminalKeyword::Literal,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        },
        ValueMember {
            member: OBJECT_LIST.as_str(),
            carrier: ValueCarrier::OrderedList,
            terminal: TerminalKeyword::Identifier,
            origin: TargetOrigin::RelationshipObject,
            reading: MultiValueReading::WholeArray,
        },
    ]
}

/// Read an instance node down to the clause 4.9 target value its attribute
/// family carries.
///
/// Tries each family's value member in turn, then a direct `@value`/`@id`
/// terminal — the shape a system sub-attribute such as `observedAt` or
/// `datasetId` takes, which carries neither `hasValue` nor `hasObject` — and
/// finally the bare node of a canonical-keyed Context Source property, which is
/// a plain JSON scalar rather than an expanded node at all.
#[must_use]
pub fn terminal_target(node: &Value) -> Option<ResolvedTarget> {
    if let Some(target) = value_members().into_iter().find_map(|member| member.target_in(node)) {
        return Some(target);
    }
    for keyword in [VALUE_KEYWORD, ID_KEYWORD, COMPACT_VALUE_KEY] {
        if let Some(raw) = node.get(keyword) {
            // clone: the caller owns the resolved value, which outlives the borrow of `node`
            return Some(ResolvedTarget {
                value: raw.clone(),
                origin: TargetOrigin::AttributeValue,
                reading: MultiValueReading::WholeArray,
            });
        }
    }
    match node {
        // clone: the caller owns the resolved value, which outlives the borrow of `node`
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Some(ResolvedTarget {
            value: node.clone(),
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        }),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// The first value node of whichever family member an attribute instance
/// carries.
///
/// This is the node a compound value's members hang off, one level above the
/// `@value` a terminal read yields.
#[must_use]
pub fn first_family_value(instance: &Value) -> Option<&Value> {
    value_members()
        .into_iter()
        .find_map(|member| instance.get(member.member).and_then(Value::as_array).and_then(|values| values.first()))
}

#[cfg(test)]
mod tests {
    use crate::filter::target_value::{TargetOrigin, terminal_target};
    use serde_json::{Value, json};

    /// The target value one expanded attribute instance carries, panicking when
    /// it carries none.
    fn target_of(instance: &Value) -> (Value, TargetOrigin) {
        let target = terminal_target(instance).expect("a target value");
        (target.value, target.origin)
    }

    /// Clause 4.9: "If the target element is a `ListProperty`, the target value
    /// is defined as the valueList array associated to such a `ListProperty`."
    #[test]
    fn a_list_property_resolves_to_the_whole_value_list() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}, {"@value": "blue"}]}]
        }));
        pretty_assertions::assert_eq!(value, json!(["red", "blue"]));
        assert_eq!(origin, TargetOrigin::AttributeValue);
    }

    /// Clause 4.9: "If the target element is a `ListRelationship`, the target
    /// object is defined as the array of objects associated (represented as
    /// URIs) to such `ListRelationship`."
    ///
    /// The core context declares `objectList` with `@container: "@list"` and no
    /// `@type: "@id"` coercion, so its members expand to `@value` nodes; a
    /// context that does coerce, and every document written in expanded form,
    /// gives `@id` nodes. Both spell the same target object.
    #[test]
    fn a_list_relationship_resolves_to_the_whole_object_list_in_either_expansion() {
        // The third member shape is the normalized representation of clause
        // 4.5.22.2: a Relationship Object wrapper carrying `hasObject`. The
        // reader used to stop at that wrapper and resolve nothing, so a `q`
        // over a `ListRelationship` silently matched no entity.
        for member in [
            json!({"@value": "urn:ngsi-ld:Part:1"}),
            json!({"@id": "urn:ngsi-ld:Part:1"}),
            json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Part:1"}]}),
            json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@value": "urn:ngsi-ld:Part:1"}]}),
        ] {
            let (value, origin) = target_of(&json!({
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [member, {"@value": "urn:ngsi-ld:Part:2"}]}]
            }));
            pretty_assertions::assert_eq!(value, json!(["urn:ngsi-ld:Part:1", "urn:ngsi-ld:Part:2"]));
            assert_eq!(origin, TargetOrigin::RelationshipObject, "a list of objects is a target object");
        }
    }

    /// A single-element `valueList` is still the array clause 4.9 names, which
    /// is what keeps the ordering and pattern operators — defined over a scalar
    /// and a String target — off it.
    #[test]
    fn a_single_element_value_list_is_still_an_array_target() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}]}]
        }));
        pretty_assertions::assert_eq!(value, json!(["red"]));
    }

    /// Clause 4.9's `equal`: "Is included in the target value, if the latter is
    /// an array". A Property whose value the client wrote as an array expands to
    /// one `hasValue` node per element, and all of them are the target value.
    #[test]
    fn an_array_valued_property_resolves_to_every_element() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "a"}, {"@value": "b"}, {"@value": "c"}]
        }));
        pretty_assertions::assert_eq!(value, json!(["a", "b", "c"]));
    }

    /// A scalar Property value stays a scalar: only a client-written array
    /// expands to several value nodes.
    #[test]
    fn a_scalar_property_resolves_to_the_bare_value() {
        let (value, _) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}));
        pretty_assertions::assert_eq!(value, json!(20));
    }

    /// Clause 4.9: "If the target element is a `VocabProperty`, the target value
    /// shall be expanded according to the @context", and the `equal` operator
    /// matches it when the operand "is included in the target value, if the
    /// latter is an array" — so every term is a target, not only the first.
    #[test]
    fn a_multi_term_vocab_property_resolves_to_every_term() {
        let (value, origin) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasVocab": [{"@id": "https://example.com/Male"}, {"@id": "https://example.com/Other"}]
        }));
        pretty_assertions::assert_eq!(value, json!(["https://example.com/Male", "https://example.com/Other"]));
        assert_eq!(origin, TargetOrigin::AttributeValue, "a VocabProperty is not a Relationship");
    }

    /// Clause 4.9: "If the target element is a `LanguageProperty`, and no target
    /// language is specified, the target value is defined as a value from any of
    /// the key-value pairs held within the languageMap."
    #[test]
    fn a_language_property_with_no_target_language_resolves_to_every_language() {
        let (value, _) = target_of(&json!({
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                {"@language": "en", "@value": "Door"},
                {"@language": "de", "@value": "Tuer"}
            ]
        }));
        pretty_assertions::assert_eq!(value, json!(["Door", "Tuer"]));
    }

    /// Clause 4.9: "If the target element is a Relationship, the target object
    /// is defined as the object associated (represented as a URI or array of
    /// URIs) to such Relationship."
    #[test]
    fn a_relationship_resolves_to_its_target_object() {
        let (value, origin) = target_of(&json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Parking:1"}]}));
        pretty_assertions::assert_eq!(value, json!("urn:ngsi-ld:Parking:1"));
        assert_eq!(origin, TargetOrigin::RelationshipObject);
    }

    /// An expanded structured Property value carries no `@value` of its own, so
    /// it is not a terminal target: a path descends into its member nodes
    /// instead.
    #[test]
    fn a_structured_property_value_resolves_no_terminal_target() {
        assert!(
            terminal_target(&json!({
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.com/city": [{"@value": "Berlin"}]}]
            }))
            .is_none()
        );
    }

    /// A system sub-attribute such as `observedAt` carries neither `hasValue`
    /// nor `hasObject`; its bare `@value` is the target the path terminates at.
    #[test]
    fn a_bare_value_node_is_its_own_target() {
        let (value, _) = target_of(&json!({"@value": "2024-06-01T00:00:00Z"}));
        pretty_assertions::assert_eq!(value, json!("2024-06-01T00:00:00Z"));
    }
}
