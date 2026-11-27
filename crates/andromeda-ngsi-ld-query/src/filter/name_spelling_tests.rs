//! How a `q` spells the three kinds of name, a term, a prefixed name (clause 4.6.2) and an absolute IRI: unquoted, or as a `quotedStr` whose value is the name.
//!
//! Unquoted, a name runs through `-`, `:`, `/` and `#` wherever an unquoted colon has no meaning of its own, and is exactly the clause 4.9 `AttrName` or `EntityType` at the head of a hop's braces and in its type list, where an unquoted colon ends the list. The quotes only delimit, and the writer quotes exactly the names that would not read back as themselves unquoted where they stand.

use crate::{
    filter::{
        Filter,
        attribute_path::AttributePath,
        equality_value::EqualityValue,
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
        query_term::{OrderingOperator, QueryTerm, SetMembership},
        scalar::Scalar,
        target_language::TargetLanguage,
        trailing_group::{MemberPath, TrailingGroup},
        value_path::ValuePath,
    },
    name_stage::Written,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_number::JsonNumber;

/// The attribute name `text`.
fn name(text: &str) -> AttributeName {
    text.parse().unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// The plain path naming the attribute `text` alone.
fn plain(text: &str) -> AttributePath<Written> {
    AttributePath::Local(ValuePath::simple(name(text)))
}

/// A hop through `relationship` into an Entity of one of `types`, reading `inner` there.
fn hop(relationship: &str, types: &[&str], inner: AttributePath<Written>) -> AttributePath<Written> {
    AttributePath::Linked {
        relationship: name(relationship),
        guard_types: types
            .iter()
            .map(|text| text.parse::<EntityTypeName>().unwrap_or_else(|error| panic!("{text}: {error}")))
            .collect(),
        inner: Box::new(inner),
    }
}

/// The term ordering `path` above the number `value`.
fn above(path: AttributePath<Written>, value: i32) -> Filter<Written> {
    Filter::Term(QueryTerm::Ordering {
        path,
        operator: OrderingOperator::Greater,
        value: Scalar::Number(JsonNumber::from(value)),
    })
}

/// The term comparing `path` with `value` by `==`.
fn equals(path: AttributePath<Written>, value: Scalar) -> Filter<Written> {
    Filter::Term(QueryTerm::Equality {
        path,
        membership: SetMembership::Within,
        value: EqualityValue::Scalar(value),
    })
}

/// The number `value` as an operand.
fn number(value: i32) -> Scalar {
    Scalar::Number(JsonNumber::from(value))
}

/// The path naming `attribute` with the trailing group `trailing`.
fn trailed(attribute: &str, trailing: TrailingGroup) -> AttributePath<Written> {
    AttributePath::Local(ValuePath {
        attribute: name(attribute),
        sub_attributes: Vec::new(),
        trailing: Some(trailing),
    })
}

/// The filter `text` parses to.
fn parsed(text: &str) -> Filter<Written> {
    text.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{text}: {error:?}"))
}

/// The refusal of `text`.
fn refusal(text: &str) -> FilterError {
    text.parse::<Filter<Written>>().expect_err("outside the language")
}

/// The path naming `attribute` with the sub-attributes `sub_attributes`.
fn dotted(attribute: &str, sub_attributes: &[&str]) -> AttributePath<Written> {
    AttributePath::Local(ValuePath {
        attribute: name(attribute),
        sub_attributes: sub_attributes.iter().map(|text| name(text)).collect(),
        trailing: None,
    })
}

/// Every accepted spelling, the tree it reads as and the text the tree is written back as.
fn accepted() -> Vec<(&'static str, Filter<Written>, &'static str)> {
    [accepted_paths(), accepted_hops(), accepted_languages()].concat()
}

/// The accepted spellings of a path that crosses no hop.
fn accepted_paths() -> Vec<(&'static str, Filter<Written>, &'static str)> {
    vec![
        ("speed>50", above(plain("speed"), 50), "speed>50"),
        ("\"ex:speed\">50", above(plain("ex:speed"), 50), "ex:speed>50"),
        ("ex:speed>50", above(plain("ex:speed"), 50), "ex:speed>50"),
        (
            "\"https://example.org/speed\">50",
            above(plain("https://example.org/speed"), 50),
            "\"https://example.org/speed\">50",
        ),
        ("\"urn:x:weight\">1000", above(plain("urn:x:weight"), 1000), "urn:x:weight>1000"),
        ("urn:xq:weight>1000", above(plain("urn:xq:weight"), 1000), "urn:xq:weight>1000"),
        ("\"urn:x:a.b\">1", above(plain("urn:x:a.b"), 1), "\"urn:x:a.b\">1"),
        ("\"speed\">50", above(plain("speed"), 50), "speed>50"),
        ("\"\\u0073peed\">50", above(plain("speed"), 50), "speed>50"),
        ("ex:speed.observedAt>1", above(dotted("ex:speed", &["observedAt"]), 1), "ex:speed.observedAt>1"),
        (
            "a.\"ex:b\".\"https://example.org/c\"[d.\"ex:e\"]==1",
            equals(
                AttributePath::Local(ValuePath {
                    attribute: name("a"),
                    sub_attributes: vec![name("ex:b"), name("https://example.org/c")],
                    trailing: Some(TrailingGroup::Members(MemberPath::new(vec![name("d"), name("ex:e")]))),
                }),
                number(1),
            ),
            "a.ex:b.\"https://example.org/c\"[d.ex:e]==1",
        ),
        (
            "address[ex:zip]==\"x\"",
            equals(
                trailed("address", TrailingGroup::Members(MemberPath::new(vec![name("ex:zip")]))),
                Scalar::Str("x".to_owned()),
            ),
            "address[ex:zip]==\"x\"",
        ),
    ]
}

/// The accepted spellings of a path crossing a hop.
fn accepted_hops() -> Vec<(&'static str, Filter<Written>, &'static str)> {
    vec![
        (
            "sensor{\"ex:humidity\"}==40",
            equals(hop("sensor", &[], plain("ex:humidity")), number(40)),
            "sensor{\"ex:humidity\"}==40",
        ),
        (
            "sensor{ex:humidity}==40",
            equals(hop("sensor", &["ex"], plain("humidity")), number(40)),
            "sensor{ex:humidity}==40",
        ),
        (
            "sensor{ex:Device:humidity}==40",
            equals(hop("sensor", &["ex"], plain("Device:humidity")), number(40)),
            "sensor{ex:Device:humidity}==40",
        ),
        (
            "sensor{Device:ex:humidity}==40",
            equals(hop("sensor", &["Device"], plain("ex:humidity")), number(40)),
            "sensor{Device:ex:humidity}==40",
        ),
        (
            "sensor{\"ex:Device\":humidity}==40",
            equals(hop("sensor", &["ex:Device"], plain("humidity")), number(40)),
            "sensor{\"ex:Device\":humidity}==40",
        ),
        (
            "sensor{\"https://example.org/Device\",\"ex:Station\":\"ex:humidity\"}==40",
            equals(hop("sensor", &["https://example.org/Device", "ex:Station"], plain("ex:humidity")), number(40)),
            "sensor{\"https://example.org/Device\",\"ex:Station\":ex:humidity}==40",
        ),
        (
            "\"ex:sensor\"{Device:\"https://example.org/humidity\"}==40",
            equals(hop("ex:sensor", &["Device"], plain("https://example.org/humidity")), number(40)),
            "ex:sensor{Device:\"https://example.org/humidity\"}==40",
        ),
        (
            "ex:sensor{Device:humidity}==40",
            equals(hop("ex:sensor", &["Device"], plain("humidity")), number(40)),
            "ex:sensor{Device:humidity}==40",
        ),
        (
            "sensor{humidity.ex:raw}==40",
            equals(hop("sensor", &[], dotted("humidity", &["ex:raw"])), number(40)),
            "sensor{humidity.ex:raw}==40",
        ),
        (
            "rel{\"ex:link\"{name}}==\"x\"",
            equals(hop("rel", &[], hop("ex:link", &[], plain("name"))), Scalar::Str("x".to_owned())),
            "rel{\"ex:link\"{name}}==\"x\"",
        ),
        (
            "rel{T:ex:link{name}}==\"x\"",
            equals(hop("rel", &["T"], hop("ex:link", &[], plain("name"))), Scalar::Str("x".to_owned())),
            "rel{T:ex:link{name}}==\"x\"",
        ),
        (
            "sensor{Device :humidity}==40",
            equals(hop("sensor", &["Device"], plain("humidity")), number(40)),
            "sensor{Device:humidity}==40",
        ),
        (
            "sensor{ Device : humidity }==40",
            equals(hop("sensor", &["Device"], plain("humidity")), number(40)),
            "sensor{Device:humidity}==40",
        ),
        (
            "sensor{\"Device\" : humidity}==40",
            equals(hop("sensor", &["Device"], plain("humidity")), number(40)),
            "sensor{Device:humidity}==40",
        ),
    ]
}

/// The accepted spellings of a trailing group naming a language.
fn accepted_languages() -> Vec<(&'static str, Filter<Written>, &'static str)> {
    vec![
        (
            "label[en-GB]==\"x\"",
            equals(
                trailed("label", TrailingGroup::Language("en-GB".parse::<TargetLanguage>().expect("a tag"))),
                Scalar::Str("x".to_owned()),
            ),
            "label[en-GB]==\"x\"",
        ),
        (
            "label[de-*-DE]==\"x\"",
            equals(
                trailed("label", TrailingGroup::Language("de-*-DE".parse::<TargetLanguage>().expect("a range"))),
                Scalar::Str("x".to_owned()),
            ),
            "label[de-*-DE]==\"x\"",
        ),
        (
            "label[*]==\"x\"",
            equals(trailed("label", TrailingGroup::AllLanguages), Scalar::Str("x".to_owned())),
            "label[*]==\"x\"",
        ),
    ]
}

#[test]
fn a_name_reads_alike_unquoted_or_as_a_quoted_string_whose_value_is_the_name() {
    for (text, tree, _) in accepted() {
        pretty_assertions::assert_eq!(parsed(text), tree, "{text}");
    }
}

#[test]
fn every_name_is_written_back_unquoted_exactly_where_it_reads_back_unquoted_as_itself() {
    for (text, _, rendered) in accepted() {
        let written = parsed(text).to_string();
        pretty_assertions::assert_eq!(written, rendered, "{text}");
        pretty_assertions::assert_eq!(parsed(&written), parsed(text), "{text} reads back from {written}");
    }
}

/// Clause 4.23.3 reads an `orderBy` term as a value path on its own.
#[test]
fn a_prefixed_name_opens_a_value_path_standing_on_its_own_quoted_or_not() {
    for text in ["\"ex:speed\".observedAt", "ex:speed.observedAt"] {
        let path = text.parse::<ValuePath<Written>>().expect("a value path");
        pretty_assertions::assert_eq!(
            path,
            ValuePath {
                attribute: name("ex:speed"),
                sub_attributes: vec![name("observedAt")],
                trailing: None,
            },
            "{text}"
        );
        pretty_assertions::assert_eq!(path.to_string(), "ex:speed.observedAt", "{text}");
    }
}

/// Wherever a colon after a name means nothing of its own, an unquoted name holding colons reads as the same name quoted.
#[test]
fn an_unquoted_name_holding_a_colon_reads_as_the_same_name_quoted() {
    for (unquoted, quoted) in [
        ("ex:speed>50", "\"ex:speed\">50"),
        ("urn:x:weight>1000", "\"urn:x:weight\">1000"),
        ("urn:x:Vehicle/a#speed>1", "\"urn:x:Vehicle/a#speed\">1"),
        ("a.ex:b==1", "a.\"ex:b\"==1"),
        ("a[ex:b]==1", "a[\"ex:b\"]==1"),
        ("sensor{T:ex:humidity}==40", "sensor{T:\"ex:humidity\"}==40"),
        ("ex:sensor{Device:humidity}==40", "\"ex:sensor\"{Device:humidity}==40"),
        ("sensor{T1,ex:T2:b}==1", "sensor{T1,ex:\"T2:b\"}==1"),
    ] {
        pretty_assertions::assert_eq!(parsed(unquoted), parsed(quoted), "{unquoted}");
    }
}

/// The colon after an unquoted first name ends the list, so the comma after the name the list's colon reads is where the closing brace was required.
#[test]
fn a_comma_after_the_name_a_list_colon_reads_is_refused_where_the_brace_was_required() {
    let error = refusal("sensor{ex:A,Store:category}");
    assert!(
        matches!(
            error,
            FilterError::Expected {
                expected: Expectation::Punctuation(Punctuation::CloseBrace),
                found: Found::Character(','),
                position: 11
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn a_quoted_value_that_is_no_name_is_refused_as_an_invalid_attribute_name() {
    for text in ["\"bad name\">1", "\"a\\\\b\">1"] {
        let error = refusal(text);
        assert!(matches!(error, FilterError::InvalidAttributeName(_)), "{text}: got {error:?}");
    }
    let error = refusal("\"\">1");
    assert!(matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Empty)), "got {error:?}");
}

/// A bare name is validated as the same name quoted, so a run the clause 4.6.2 grammar refuses is refused alike in both spellings.
#[test]
fn an_unquoted_name_outside_the_name_grammar_is_refused_as_the_same_name_quoted() {
    for text in ["a-b", "a/b", "a#b", "speed-limit"] {
        for written in [format!("{text}>1"), format!("\"{text}\">1")] {
            let error = refusal(&written);
            assert!(
                matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Invalid)),
                "{written}: got {error:?}"
            );
        }
    }
}

/// An unquoted name ends at the first character outside its set, and none opens at a digit, `_`, `:`, `/`, `#` or `-`.
#[test]
fn an_unquoted_name_ends_at_the_first_character_outside_its_set_and_opens_only_at_a_letter() {
    for (text, character, position) in [("a@b>1", '@', 1), ("a+b>1", '+', 1), ("urn:x:a=b>1", '=', 7)] {
        let error = refusal(text);
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: found, position: at } if found == character && at == position),
            "{text}: got {error:?}"
        );
    }
    for (text, character) in [(":a>1", ':'), ("/a>1", '/'), ("_a>1", '_'), ("#a>1", '#'), ("-a>1", '-')] {
        let error = refusal(text);
        assert!(
            matches!(error, FilterError::Expected { expected: Expectation::AttributeName, found: Found::Character(found), position: 0 } if found == character),
            "{text}: got {error:?}"
        );
    }
}

#[test]
fn a_quoted_name_left_open_is_an_unterminated_string() {
    for text in ["\"ex:speed>50", "sensor{\"ex:humidity}==40"] {
        let error = refusal(text);
        assert!(matches!(error, FilterError::UnterminatedString), "{text}: got {error:?}");
    }
}

/// A name is written quoted where its unquoted text would end early or read as another filter: a dot, a query mark or whitespace it holds, or a colon at the head of a hop's braces.
#[test]
fn a_name_is_written_quoted_exactly_where_it_would_not_read_back_unquoted() {
    for (filter, written) in [
        (above(plain("urn:x:weight"), 1000), "urn:x:weight>1000"),
        (above(plain("urn:x:a;b"), 5), "\"urn:x:a;b\">5"),
        (above(plain("urn:x:a.b"), 5), "\"urn:x:a.b\">5"),
        (above(plain("urn:x:a(b)"), 5), "\"urn:x:a(b)\">5"),
        (above(dotted("a", &["urn:x:a=b"]), 5), "a.\"urn:x:a=b\">5"),
        (above(hop("ex:sensor", &[], plain("humidity")), 5), "ex:sensor{humidity}>5"),
        (above(hop("sensor", &[], plain("ex:humidity")), 5), "sensor{\"ex:humidity\"}>5"),
        (
            above(hop("sensor", &["ex:Device"], plain("ex:humidity")), 5),
            "sensor{\"ex:Device\":ex:humidity}>5",
        ),
        (
            above(hop("sensor", &["Device", "urn:x:T"], plain("urn:x:a.b")), 5),
            "sensor{Device,\"urn:x:T\":\"urn:x:a.b\"}>5",
        ),
        (above(hop("rel", &[], hop("ex:link", &[], plain("name"))), 5), "rel{\"ex:link\"{name}}>5"),
        (
            above(hop("rel", &["T"], hop("ex:link", &[], plain("ex:name"))), 5),
            "rel{T:ex:link{\"ex:name\"}}>5",
        ),
    ] {
        pretty_assertions::assert_eq!(filter.to_string(), written);
        pretty_assertions::assert_eq!(parsed(written), filter, "{written}");
    }
}
