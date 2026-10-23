use crate::{aliased_deserializer::AliasedDeserializer, core_term_aliases::CoreTermAliases};
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

/// `text` decoded as a `T` through [`aliases`].
fn read<T: DeserializeOwned>(text: &str) -> T {
    let aliases = aliases();
    let mut reader = serde_json::Deserializer::from_str(text);
    T::deserialize(AliasedDeserializer::new(&mut reader, &aliases)).expect("the fixture decodes")
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
    T::deserialize(AliasedDeserializer::new(&mut reader, &aliases))
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
