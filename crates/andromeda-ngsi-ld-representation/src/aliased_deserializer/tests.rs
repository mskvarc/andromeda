use crate::{
    aliased_deserializer::AliasedDeserializer,
    compact_node::context_reading::ContextReading,
    container_terms::{ContainerTerms, TermContainer},
    core_term_aliases::CoreTermAliases,
};
use andromeda_json_literal::JsonLiteral;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// The keyword spellings, with `kind` aliasing `type`, `v` aliasing `value`
/// and `Prop` aliasing the `Property` class.
fn aliases() -> CoreTermAliases {
    let mut aliases = CoreTermAliases::keyword_spellings().clone(); // clone: the fixture extends the shared table
    aliases.alias("kind", "type");
    aliases.alias("v", "value");
    aliases.alias("Prop", "Property");
    aliases
}

/// The containers a `@set` alias of `type`, an `@index` map under `speed`, a
/// `@set` term that reaches no core term and a `@nest` alias give.
fn containers() -> ContainerTerms {
    let mut containers = ContainerTerms::none();
    containers.contain("kinds", TermContainer::Set);
    containers.contain("speed", TermContainer::Map);
    containers.contain("tags", TermContainer::Set);
    containers.contain("n", TermContainer::Nest);
    containers
}

/// `text` decoded as a `T` through [`aliases`] and no container.
fn read<T: DeserializeOwned>(text: &str) -> T {
    let aliases = aliases();
    let mut reader = serde_json::Deserializer::from_str(text);
    T::deserialize(AliasedDeserializer::new(&mut reader, ContextReading::new(&aliases, &ContainerTerms::none()))).expect("the fixture decodes")
}

/// `text` decoded as a `T` through [`aliases`] and [`containers`].
fn read_contained<T: DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    let mut aliases = aliases();
    aliases.alias("kinds", "type");
    let containers = containers();
    let mut reader = serde_json::Deserializer::from_str(text);
    T::deserialize(AliasedDeserializer::new(&mut reader, ContextReading::new(&aliases, &containers)))
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    value: u8,
}

#[test]
fn keys_are_read_under_their_core_terms() {
    pretty_assertions::assert_eq!(
        read::<Node>(r#"{"@id": "urn:ngsi-ld:T:1", "kind": "Prop", "v": 1}"#),
        Node {
            id: "urn:ngsi-ld:T:1".to_owned(),
            kind: "Property".to_owned(),
            value: 1,
        }
    );
}

/// Only a string of a `type` member names a class; the same string anywhere
/// else is data.
#[test]
fn nested_objects_are_renamed_at_every_depth() {
    let read = read::<BTreeMap<String, BTreeMap<String, Vec<String>>>>(r#"{"outer": {"kind": ["Prop", "Vehicle"], "v": ["Prop"]}}"#);
    let outer = read.get("outer").expect("the outer member");
    pretty_assertions::assert_eq!(outer.get("type"), Some(&vec!["Property".to_owned(), "Vehicle".to_owned()]));
    pretty_assertions::assert_eq!(outer.get("value"), Some(&vec!["Prop".to_owned()]));
}

#[derive(Debug, Deserialize)]
struct Raw {
    value: Box<RawValue>,
}

#[test]
fn a_raw_value_is_captured_unrenamed() {
    pretty_assertions::assert_eq!(read::<Raw>(r#"{"v": {"kind": "Prop"}}"#).value.get(), r#"{"kind": "Prop"}"#);
}

#[derive(Debug, Deserialize)]
struct Literal {
    value: JsonLiteral,
    #[serde(default)]
    maybe: Option<JsonLiteral>,
}

#[test]
fn a_json_literal_member_keeps_its_aliased_keys() {
    let read = read::<Literal>(r#"{"v": {"kind": "Prop"}, "maybe": ["v", {"@id": 1}]}"#);
    pretty_assertions::assert_eq!(read.value, JsonLiteral::admit(r#"{"kind": "Prop"}"#).expect("a literal"));
    pretty_assertions::assert_eq!(read.maybe, Some(JsonLiteral::admit(r#"["v", {"@id": 1}]"#).expect("a literal")));
}

/// `text` decoded as a `T` through [`aliases`], or the reader's refusal.
fn attempt<T: DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    let aliases = aliases();
    let mut reader = serde_json::Deserializer::from_str(text);
    T::deserialize(AliasedDeserializer::new(&mut reader, ContextReading::new(&aliases, &ContainerTerms::none())))
}

/// A resource whose one member reads as a unit.
#[derive(Debug, Deserialize)]
struct UnitMember {
    #[serde(rename = "value")]
    _value: (),
}

/// Clause 5.2.1: no member admits a JSON `null`, whether the type reads it as
/// an optional value, as a unit or as any JSON value, at any depth.
#[test]
fn a_json_null_is_refused_at_every_depth() {
    for text in [r#"{"value": null}"#, r#"{"outer": {"value": null}}"#, r#"{"outer": [1, null]}"#] {
        let refusal = attempt::<BTreeMap<String, serde_json::Value>>(text).expect_err("a JSON null is no value");
        assert!(refusal.to_string().starts_with("invalid type: null"), "{text}: {refusal}");
    }
    assert!(attempt::<BTreeMap<String, Option<u8>>>(r#"{"value": null}"#).is_err());
    assert!(attempt::<UnitMember>(r#"{"value": null}"#).is_err());
    pretty_assertions::assert_eq!(
        attempt::<BTreeMap<String, Option<u8>>>(r#"{"value": 1}"#).ok(),
        Some(BTreeMap::from([("value".to_owned(), Some(1))]))
    );
}

/// A member the resource does not read is skipped whatever it holds.
#[test]
fn a_member_the_resource_skips_is_not_read() {
    #[derive(Debug, Deserialize)]
    struct Skipping {
        #[serde(rename = "value")]
        _value: u8,
    }

    for text in [r#"{"value": 1, "status": null}"#, r#"{"value": 1, "status": {"a": [null, "x"]}}"#] {
        assert!(attempt::<Skipping>(text).is_ok(), "{text}");
    }
}

/// What a body captures as JSON of its own is data, a `null` in it included.
#[test]
fn a_json_null_inside_a_captured_literal_is_data() {
    let read = read::<Literal>(r#"{"v": {"a": null}, "maybe": [null]}"#);
    pretty_assertions::assert_eq!(read.value, JsonLiteral::admit(r#"{"a": null}"#).expect("a literal"));
    pretty_assertions::assert_eq!(read.maybe, Some(JsonLiteral::admit("[null]").expect("a literal")));
}

/// A member name a number decodes from is read as written.
#[test]
fn numeric_member_names_and_numbers_are_read_as_written() {
    pretty_assertions::assert_eq!(read::<BTreeMap<u32, f64>>(r#"{"1": 2.5}"#), BTreeMap::from([(1, 2.5)]));
}

/// A node whose members a context with containers writes in the shape
/// compaction gives them.
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Contained {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    speed: Option<Speed>,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    brand: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Speed {
    value: u8,
}

#[test]
fn an_array_of_one_under_a_set_alias_of_type_is_read_as_the_type() {
    let read = read_contained::<Contained>(r#"{"id": "urn:x", "kinds": ["Prop"]}"#).expect("the node decodes");
    pretty_assertions::assert_eq!(read.kind, "Property");
}

#[test]
fn an_array_of_one_under_a_set_term_that_reaches_no_core_term_is_its_element() {
    let read = read_contained::<Contained>(r#"{"id": "urn:x", "type": "Vehicle", "tags": ["fast"]}"#).expect("the node decodes");
    pretty_assertions::assert_eq!(read.tags.as_deref(), Some("fast"));
}

#[test]
fn an_array_of_several_under_a_set_term_is_read_as_written() {
    assert!(read_contained::<Contained>(r#"{"id": "urn:x", "type": "Vehicle", "tags": ["a", "b"]}"#).is_err());
    pretty_assertions::assert_eq!(
        read_contained::<BTreeMap<String, Vec<String>>>(r#"{"tags": ["a", "b"]}"#)
            .expect("the map decodes")
            .get("tags"),
        Some(&vec!["a".to_owned(), "b".to_owned()])
    );
}

#[test]
fn an_object_of_one_key_under_a_map_container_is_the_value_under_it() {
    let read = read_contained::<Contained>(r#"{"id": "urn:x", "type": "Vehicle", "speed": {"@none": {"value": 5}}}"#).expect("the node decodes");
    pretty_assertions::assert_eq!(read.speed, Some(Speed { value: 5 }));
}

#[test]
fn an_object_of_several_keys_under_a_map_container_is_read_as_written() {
    assert!(read_contained::<Contained>(r#"{"id": "urn:x", "type": "Vehicle", "speed": {"a": {"value": 1}, "b": {"value": 2}}}"#).is_err());
}

#[test]
fn the_members_under_a_nest_alias_belong_to_the_object_holding_it() {
    let read = read_contained::<Contained>(r#"{"id": "urn:x", "n": {"type": "Vehicle", "brand": "a", "n": {"model": "b"}}}"#).expect("the node decodes");
    pretty_assertions::assert_eq!(
        (read.kind.as_str(), read.brand.as_deref(), read.model.as_deref()),
        ("Vehicle", Some("a"), Some("b"))
    );
}

#[test]
fn a_nest_alias_holding_no_object_is_read_as_written() {
    pretty_assertions::assert_eq!(
        read_contained::<BTreeMap<String, String>>(r#"{"n": "x"}"#).expect("the map decodes"),
        BTreeMap::from([("n".to_owned(), "x".to_owned())])
    );
}

#[test]
fn a_container_is_undone_at_every_depth_of_a_nest() {
    let read = read_contained::<Contained>(r#"{"id": "urn:x", "n": {"kinds": ["Prop"], "speed": {"@none": {"value": 7}}}}"#).expect("the node decodes");
    pretty_assertions::assert_eq!((read.kind.as_str(), read.speed), ("Property", Some(Speed { value: 7 })));
}

#[test]
fn a_value_captured_as_json_is_not_reshaped_inside() {
    let read = read_contained::<Literal>(r#"{"v": {"tags": ["a"]}, "maybe": ["x"]}"#).expect("the literal decodes");
    pretty_assertions::assert_eq!(read.value, JsonLiteral::admit(r#"{"tags": ["a"]}"#).expect("a literal"));
}

#[test]
fn a_json_null_under_a_container_term_is_refused() {
    assert!(read_contained::<Contained>(r#"{"id": "urn:x", "type": "Vehicle", "tags": [null]}"#).is_err());
}
