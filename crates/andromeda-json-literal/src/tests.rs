use crate::{
    JsonLiteral,
    admission::JsonLiteralError,
    piece::{JsonKind, JsonPiece},
};
use andromeda_ngsi_ld_number::json_value::{READER_NESTING_LIMIT, json_pointer::JsonPointerSegment};
use std::thread;

fn literal(text: &str) -> JsonLiteral {
    JsonLiteral::admit(text).expect("an admissible literal")
}

/// Admission drops whitespace and keeps the members in written order.
#[test]
fn members_keep_their_written_order() {
    pretty_assertions::assert_eq!(literal(r#"{ "b" : 1, "a" : [ 2 , true, null ] }"#).text(), r#"{"b":1,"a":[2,true,null]}"#);
    pretty_assertions::assert_eq!(
        serde_json::to_string(&literal(r#"{"b":1,"a":2}"#)).expect("a written literal"),
        r#"{"b":1,"a":2}"#
    );
}

/// Clause 4.6.3 keeps a number's value, not its spelling.
#[test]
fn numbers_are_canonical() {
    pretty_assertions::assert_eq!(literal(r#"{"n":1.50}"#).text(), r#"{"n":1.5}"#);
    pretty_assertions::assert_eq!(literal(r#"{"n":1e2}"#).text(), r#"{"n":100}"#);
    pretty_assertions::assert_eq!(literal("[-0, 0.100000000000000005]").text(), "[0,0.100000000000000005]");
}

/// A number the broker cannot keep exactly is refused where it stands.
#[test]
fn a_number_out_of_range_is_refused_at_its_location() {
    let Err(JsonLiteralError::Number { path, .. }) = JsonLiteral::admit(r#"{"a":[1,1e999999999999]}"#) else {
        panic!("an out-of-range number is refused");
    };
    pretty_assertions::assert_eq!(path.to_string(), "/a/1");
}

/// IETF RFC 8259 section 4 gives a repeated member name no meaning.
#[test]
fn duplicate_member_names_are_refused() {
    let Err(JsonLiteralError::DuplicateMember { path }) = JsonLiteral::admit(r#"{"x":{"a":1,"a":2}}"#) else {
        panic!("a repeated member name is refused");
    };
    pretty_assertions::assert_eq!(
        path.segments(),
        [JsonPointerSegment::Member(Box::from("x")), JsonPointerSegment::Member(Box::from("a"))]
    );
}

/// Text that is not one JSON value is refused.
#[test]
fn malformed_text_is_refused() {
    for text in ["", "{", "[1,]", "1 2", "NaN"] {
        assert!(matches!(JsonLiteral::admit(text), Err(JsonLiteralError::Syntax { .. })), "{text}");
    }
}

/// A literal may nest as deep as any JSON text the broker reads, and no
/// deeper.
#[test]
fn nesting_is_bounded_by_the_reader_limit() {
    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(JsonLiteral::admit(&nested(READER_NESTING_LIMIT)).is_ok());
    assert!(matches!(
        JsonLiteral::admit(&nested(READER_NESTING_LIMIT + 1)),
        Err(JsonLiteralError::TooDeep { .. })
    ));
}

/// IETF RFC 8259 section 4: an object is unordered; arrays are ordered.
#[test]
fn equality_ignores_member_order() {
    pretty_assertions::assert_eq!(literal(r#"{"a":1,"b":{"c":[1,2],"d":"x"}}"#), literal(r#"{"b":{"d":"x","c":[1,2]},"a":1}"#));
    assert_ne!(literal("[1,2]"), literal("[2,1]"));
    assert_ne!(literal(r#"{"a":1}"#), literal(r#"{"a":1,"b":2}"#));
    assert_ne!(literal(r#"{"a":1,"b":2}"#), literal(r#"{"a":1}"#));
}

/// Clause 4.6.3: `1.0` and `1` are the same number; strings compare
/// unescaped.
#[test]
fn equality_compares_numbers_by_value() {
    pretty_assertions::assert_eq!(literal("1.0"), literal("1"));
    pretty_assertions::assert_eq!(literal(r#""a""#), literal(r#""a""#));
    assert_ne!(literal("1"), literal(r#""1""#));
    assert_ne!(literal("null"), literal("false"));
}

/// A member path is read level by level from the text.
#[test]
fn a_member_path_is_read_lazily() {
    let document = literal(r#"{"a":{"b":[3]}}"#);
    let found = document.piece().member_path(["a", "b"]).expect("the member");
    pretty_assertions::assert_eq!(found.text(), "[3]");
    pretty_assertions::assert_eq!(
        found.elements().map(|elements| elements.into_iter().map(JsonPiece::text).collect::<Vec<_>>()),
        Some(vec!["3"])
    );
    assert!(document.piece().member_path(["a", "c"]).is_none());
    assert!(document.piece().member_path(["a", "b", "c"]).is_none());
    pretty_assertions::assert_eq!(found.to_literal(), literal("[3]"));
}

/// A scalar takes no level; each array or object one more than its deepest
/// content.
#[test]
fn levels_count_the_containers_of_the_deepest_branch() {
    pretty_assertions::assert_eq!(literal("1").piece().levels(), 0);
    pretty_assertions::assert_eq!(literal("[]").piece().levels(), 1);
    pretty_assertions::assert_eq!(literal(r#"{"a":[1,{"b":[]}],"c":2}"#).piece().levels(), 4);
}

/// Each kind reads as itself and as nothing else.
#[test]
fn a_piece_reads_as_its_kind() {
    let document = literal(r#"{"s":"t\"x","n":-2.5,"b":false,"z":null}"#);
    let member = |name: &str| document.piece().member(name).expect("the member");
    pretty_assertions::assert_eq!(member("s").as_str().as_deref(), Some("t\"x"));
    pretty_assertions::assert_eq!(member("n").as_number().map(|number| number.to_string()), Some("-2.5".to_owned()));
    pretty_assertions::assert_eq!(member("b").as_bool(), Some(false));
    pretty_assertions::assert_eq!(member("z").kind(), JsonKind::Null);
    assert!(member("s").as_number().is_none() && member("n").as_str().is_none() && member("z").as_bool().is_none());
    assert!(member("s").members().is_none() && member("s").elements().is_none());
}

/// IETF RFC 7396 with the NGSI-LD Null in place of `null`.
#[test]
fn merge_patch_follows_rfc_7396() {
    let merged = literal(r#"{"a":1,"b":{"c":2}}"#)
        .merged(&literal(r#"{"b":{"c":"urn:ngsi-ld:null","d":3}}"#))
        .expect("a merged literal");
    pretty_assertions::assert_eq!(merged.text(), r#"{"a":1,"b":{"d":3}}"#);
}

/// Clause 5.5.12 removes the member the patch set to the NGSI-LD Null and
/// leaves every other one alone, including a genuine JSON `null`, which is
/// data rather than a deletion (clause 4.5.0; the fixture of ETSI test
/// `056_10_01`).
#[test]
fn a_member_set_to_the_null_sentinel_is_removed_and_the_rest_survive() {
    let stored = literal(r#"{"id":"some-id","type":"some-type","null":null,"anythingElse":"anything-else"}"#);
    let merged = stored.merged(&literal(r#"{"anythingElse":"urn:ngsi-ld:null"}"#)).expect("a merged literal");
    pretty_assertions::assert_eq!(merged.text(), r#"{"id":"some-id","type":"some-type","null":null}"#);
}

/// A member the patch names is replaced, one it omits survives, and one only
/// the patch names follows the target's members.
#[test]
fn a_named_member_is_replaced_and_an_omitted_one_survives() {
    let merged = literal(r#"{"id":"some-id","kept":1}"#)
        .merged(&literal(r#"{"added":true,"id":"another-id"}"#))
        .expect("a merged literal");
    pretty_assertions::assert_eq!(merged.text(), r#"{"id":"another-id","kept":1,"added":true}"#);
}

/// Clause 4.5.0: a JSON `null` in the patch is written as data.
#[test]
fn a_json_null_in_the_patch_is_written_as_data() {
    let merged = literal(r#"{"present":1}"#).merged(&literal(r#"{"present":null}"#)).expect("a merged literal");
    pretty_assertions::assert_eq!(merged.text(), r#"{"present":null}"#);
}

/// IETF RFC 7396 section 2 recurses into an object the target lacks, so a
/// Null nested under a new member is dropped.
#[test]
fn a_null_nested_under_a_member_the_target_lacks_is_dropped() {
    let merged = literal(r#"{"kept":1}"#)
        .merged(&literal(r#"{"added":{"a":2,"b":"urn:ngsi-ld:null"}}"#))
        .expect("a merged literal");
    pretty_assertions::assert_eq!(merged.text(), r#"{"kept":1,"added":{"a":2}}"#);
}

/// A patch that is not an object replaces the target, and a target that is
/// not an object is replaced by the patch's own object.
#[test]
fn a_non_object_on_either_side_replaces_rather_than_merges() {
    pretty_assertions::assert_eq!(
        literal("[1,2,3]").merged(&literal(r#"{"a":1}"#)).expect("a merged literal").text(),
        r#"{"a":1}"#
    );
    pretty_assertions::assert_eq!(literal(r#"{"a":1}"#).merged(&literal("[4,5]")).expect("a merged literal").text(), "[4,5]");
}

/// A literal merged into nothing loses the members set to the NGSI-LD Null,
/// at any depth of nested objects.
#[test]
fn a_settled_literal_leaves_its_deletions_out() {
    let settled = literal(r#"{"a":1,"b":"urn:ngsi-ld:null","c":{"d":"urn:ngsi-ld:null","e":null}}"#)
        .settled()
        .expect("a settled literal");
    pretty_assertions::assert_eq!(settled.text(), r#"{"a":1,"c":{"e":null}}"#);
    pretty_assertions::assert_eq!(
        literal(r#"["urn:ngsi-ld:null"]"#).settled().expect("a settled literal").text(),
        r#"["urn:ngsi-ld:null"]"#
    );
}

/// Clause 5.5.4: the Null under some member is found at any depth and
/// through arrays; a JSON `null` is data.
#[test]
fn a_sentinel_under_some_member_is_found_at_any_depth() {
    assert!(literal(r#"{"a":{"b":[{"c":"urn:ngsi-ld:null"}]}}"#).carries_null_sentinel_under_a_key());
    assert!(literal(r#"[{"c":["urn:ngsi-ld:null"]}]"#).carries_null_sentinel_under_a_key());
    assert!(!literal(r#"{"a":null}"#).carries_null_sentinel_under_a_key());
    assert!(!literal(r#"["urn:ngsi-ld:null"]"#).carries_null_sentinel_under_a_key());
}

/// The literal being the Null is the clause 4.5.24.2 deletion encoding, not
/// the nested prohibition.
#[test]
fn the_whole_value_being_the_sentinel_is_not_the_nested_prohibition() {
    let sentinel = literal(r#""urn:ngsi-ld:null""#);
    assert!(sentinel.is_null_sentinel());
    assert!(!sentinel.carries_null_sentinel_under_a_key());
    assert!(!literal("null").is_null_sentinel());
}

/// What is read back is what was admitted, and what is written is the text.
#[test]
fn a_literal_round_trips_through_serde_as_its_text() {
    let read: JsonLiteral = serde_json::from_str(r#" {"a" : [1.50, "x"]} "#).expect("a literal");
    pretty_assertions::assert_eq!(read.text(), r#"{"a":[1.5,"x"]}"#);
    pretty_assertions::assert_eq!(serde_json::to_string(&read).expect("a written literal"), read.text());
    assert!(serde_json::from_str::<JsonLiteral>(r#"{"a":1,"a":2}"#).is_err());
}

/// A literal is read through serde's buffered content too, as a flattened
/// struct or a tagged enum reads it, and from a parsed tree, with numbers
/// kept exact.
#[test]
fn a_literal_is_read_from_any_deserializer() {
    #[derive(serde::Deserialize)]
    struct Inner {
        json: JsonLiteral,
    }
    #[derive(serde::Deserialize)]
    struct Outer {
        #[serde(flatten)]
        inner: Inner,
    }
    let text = r#"{"json": {"n": 0.100000000000000005, "big": 123456789012345678901234567890, "a": [true, null, "x"], "e": {}}}"#;
    let expected = r#"{"n":0.100000000000000005,"big":123456789012345678901234567890,"a":[true,null,"x"],"e":{}}"#;
    pretty_assertions::assert_eq!(serde_json::from_str::<Outer>(text).expect("a flattened literal").inner.json.text(), expected);
    // serde's buffered content holds no 128-bit integer, so the tree is read
    // unflattened; the tree sorts its members, which equality ignores.
    let tree: serde_json::Value = serde_json::from_str(text).expect("a tree");
    pretty_assertions::assert_eq!(serde_json::from_value::<Inner>(tree).expect("a literal from a tree").json, literal(expected));
}

/// The processor's own JSON type is built from the text and read back into
/// an equal literal.
#[test]
fn a_literal_round_trips_through_the_processor_json() {
    let original = literal(r#"{"b":[1,{"c":null}],"a":"x"}"#);
    let processor = original.to_processor_json().expect("processor JSON");
    pretty_assertions::assert_eq!(JsonLiteral::from_processor_json(&processor).expect("a literal"), original);
}

/// The literal is one heap string however deep it nests, so cloning and
/// dropping it needs no stack in proportion to its depth.
#[test]
fn a_deep_literal_drops_on_a_small_stack() {
    let deep = literal(&format!("{}{}", "[".repeat(READER_NESTING_LIMIT), "]".repeat(READER_NESTING_LIMIT)));
    let handle = thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let copy = deep.clone();
            drop(deep);
            copy.text().len()
        })
        .expect("a thread");
    pretty_assertions::assert_eq!(handle.join().expect("the thread finishes"), 2 * READER_NESTING_LIMIT);
}
