use crate::compact_node::{CompactNode, NodeShape};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::{Value, json, value::RawValue};

/// The text of `value` as raw JSON, the way a request hands it over.
fn raw(value: &Value) -> Box<RawValue> {
    RawValue::from_string(value.to_string()).expect("the fixture is JSON")
}

/// A shape, summarised as text for comparison.
fn summary<'n, N: CompactNode<'n>>(node: N) -> String {
    match node.shape() {
        NodeShape::Null => "null".to_owned(),
        NodeShape::Boolean(flag) => flag.to_string(),
        NodeShape::Number(_) => "number".to_owned(),
        NodeShape::String(text) => format!("{text:?}"),
        NodeShape::Array(items) => format!("[{}]", items.into_iter().map(summary).collect::<Vec<_>>().join(",")),
        NodeShape::Object(members) => format!(
            "{{{}}}",
            members
                .into_iter()
                .map(|(name, value)| format!("{name}:{}", summary(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        NodeShape::Geometry(_) => "geometry".to_owned(),
    }
}

/// Text read in place and a parsed document read alike, member order aside:
/// a parsed object keeps its members sorted.
#[test]
fn text_and_a_parsed_document_have_the_same_shape() {
    let document = json!({"a": [1, "x\u{e9}", null, true, {"b": false}], "c": {"d": 2.5}});
    let text = raw(&document);
    let piece = andromeda_json_literal::piece::JsonPiece::new(&text);
    pretty_assertions::assert_eq!(summary(piece), summary(&document));
}

/// A member written twice counts as written last, as a parsed tree keeps it.
#[test]
fn a_member_written_twice_is_read_as_written_last() {
    let text = RawValue::from_string(r#"{"a": 1, "a": "last"}"#.to_owned()).expect("JSON text");
    let piece = andromeda_json_literal::piece::JsonPiece::new(&text);
    pretty_assertions::assert_eq!(piece.member("a").and_then(CompactNode::text).as_deref(), Some("last"));
}

/// A value of request text is admitted as a literal of its own, with the
/// canonical spelling of its numbers.
#[test]
fn a_value_of_request_text_is_admitted_as_a_literal() {
    let text = RawValue::from_string(r#"{"v": {"n": 1.50}}"#.to_owned()).expect("JSON text");
    let piece = andromeda_json_literal::piece::JsonPiece::new(&text);
    let literal = piece.member("v").expect("the member").literal().expect("readable").expect("admitted");
    pretty_assertions::assert_eq!(literal, JsonLiteral::admit(r#"{"n":1.5}"#).expect("a literal"));
    pretty_assertions::assert_eq!(literal.text(), r#"{"n":1.5}"#);
}

/// A string node decodes through the string, from text and from a tree.
#[test]
fn a_string_node_decodes_as_the_type_it_names() {
    let document = json!({"id": "urn:ngsi-ld:T:1", "bad": "not a uri"});
    let text = raw(&document);
    let piece = andromeda_json_literal::piece::JsonPiece::new(&text);
    for (member, decodes) in [("id", true), ("bad", false)] {
        let from_text = piece.member(member).expect("the member").decoded::<Identifier>().expect("readable");
        let from_tree = document[member].decoded::<Identifier>().expect("readable");
        pretty_assertions::assert_eq!(from_text.is_ok(), decodes, "{member}");
        pretty_assertions::assert_eq!(from_tree.is_ok(), decodes, "{member}");
    }
}

/// A geometry is read from the slice of text it is.
#[test]
fn a_geometry_is_read_from_its_text() {
    let text = raw(&json!({"value": {"type": "Point", "coordinates": [1.0, 2.0]}}));
    let piece = andromeda_json_literal::piece::JsonPiece::new(&text);
    let written = piece.member("value").expect("the member").read_geometry().expect("readable");
    assert!(written.is_ok(), "{written:?}");
}
