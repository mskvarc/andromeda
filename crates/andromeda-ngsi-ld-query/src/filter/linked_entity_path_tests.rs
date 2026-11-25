//! The clause 4.9 `LinkedEntityPath`, read with its type list holding one type or more and standing before a further hop or a plain path.
//!
//! As printed, the rule asks for two types or more (`1*(%x2C EntityType)`) and attaches the list only to the alternative that continues with a further hop, while EXAMPLE 14 writes one type before the compared attribute (`sensor{Device:humidity}==40`). The trees below are those of the rule read with both limits lifted, `*1(EntityType *(%x2C EntityType) %x3A) (AttrName %x7B LinkedEntityPath %x7D / ValuePath)`.

use crate::{
    filter::{
        Filter,
        attribute_path::AttributePath,
        equality_value::EqualityValue,
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
        query_term::{QueryTerm, SetMembership},
        scalar::Scalar,
        trailing_group::{MemberPath, TrailingGroup},
        value_path::ValuePath,
    },
    name_stage::Written,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
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

/// The term comparing `path` with the number `value` by `==`.
fn equals(path: AttributePath<Written>, value: i32) -> Filter<Written> {
    Filter::Term(QueryTerm::Equality {
        path,
        membership: SetMembership::Within,
        value: EqualityValue::Scalar(Scalar::Number(JsonNumber::from(value))),
    })
}

/// The term testing that `path` exists.
fn exists(path: AttributePath<Written>) -> Filter<Written> {
    Filter::Term(QueryTerm::Exists(path))
}

/// The filter `text` parses to.
fn parsed(text: &str) -> Filter<Written> {
    text.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{text}: {error:?}"))
}

/// Every shape the rule derives, with the tree it reads as and the text the tree renders as.
fn accepted() -> [(&'static str, Filter<Written>, &'static str); 11] {
    [
        (
            "sensor{humidity}==40",
            equals(hop("sensor", &[], plain("humidity")), 40),
            "sensor{humidity}==40",
        ),
        (
            "sensor{Device:humidity}==40",
            equals(hop("sensor", &["Device"], plain("humidity")), 40),
            "sensor{Device:humidity}==40",
        ),
        ("a{T1,T2:b}==1", equals(hop("a", &["T1", "T2"], plain("b")), 1), "a{T1,T2:b}==1"),
        ("a{T:b{c}}", exists(hop("a", &["T"], hop("b", &[], plain("c")))), "a{T:b{c}}"),
        ("a{T1,T2:b{c}}", exists(hop("a", &["T1", "T2"], hop("b", &[], plain("c")))), "a{T1,T2:b{c}}"),
        ("a{T1:b{T2:c}}", exists(hop("a", &["T1"], hop("b", &["T2"], plain("c")))), "a{T1:b{T2:c}}"),
        ("a{T1,T2,T3:b}==1", equals(hop("a", &["T1", "T2", "T3"], plain("b")), 1), "a{T1,T2,T3:b}==1"),
        (
            "a{T:b[c]}==1",
            equals(
                hop(
                    "a",
                    &["T"],
                    AttributePath::Local(ValuePath {
                        attribute: name("b"),
                        sub_attributes: Vec::new(),
                        trailing: Some(TrailingGroup::Members(MemberPath::new(vec![name("c")]))),
                    }),
                ),
                1,
            ),
            "a{T:b[c]}==1",
        ),
        (
            "a{T:b.c}==1",
            equals(
                hop(
                    "a",
                    &["T"],
                    AttributePath::Local(ValuePath {
                        attribute: name("b"),
                        sub_attributes: vec![name("c")],
                        trailing: None,
                    }),
                ),
                1,
            ),
            "a{T:b.c}==1",
        ),
        ("a{ T1 , T2 : b }==1", equals(hop("a", &["T1", "T2"], plain("b")), 1), "a{T1,T2:b}==1"),
        ("a{T:b}", exists(hop("a", &["T"], plain("b"))), "a{T:b}"),
    ]
}

#[test]
fn a_type_list_of_one_type_or_more_stands_before_a_further_hop_or_a_plain_path() {
    for (text, tree, _) in accepted() {
        pretty_assertions::assert_eq!(parsed(text), tree, "{text}");
    }
}

#[test]
fn a_hop_renders_its_type_list_as_written_and_drops_the_whitespace_between_its_marks() {
    for (text, _, rendered) in accepted() {
        pretty_assertions::assert_eq!(parsed(text).to_string(), rendered, "{text}");
    }
}

#[test]
fn a_type_list_missing_a_type_a_colon_or_an_attribute_after_it_is_refused_where_the_grammar_required_one() {
    for (text, expected, found, position) in [
        ("a{}", Expectation::AttributeName, Found::Character('}'), 2),
        ("a{T1,}", Expectation::EntityType, Found::Character('}'), 5),
        ("a{T1,T2}", Expectation::GuardTerminator, Found::Character('}'), 7),
        ("a{T:}", Expectation::AttributeName, Found::Character('}'), 4),
        ("a{T:b{c==1}}", Expectation::Punctuation(Punctuation::CloseBrace), Found::Character('='), 7),
    ] {
        let error = text.parse::<Filter<Written>>().expect_err("outside the rule");
        assert!(
            matches!(&error, FilterError::Expected { expected: refused, found: stood, position: at } if *refused == expected && *stood == found && *at == position),
            "{text}: got {error:?}"
        );
    }
}
