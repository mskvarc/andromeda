//! The path half of a lowered Query Term.

use crate::filter::{
    expression::Expr,
    instance_metadata::InstanceMetadata,
    language_matching::LanguageMatching,
    parser::parse,
    reading::{
        filter_reading::FilterReading,
        hop::Hop,
        operand::Operand,
        scalar_test::ScalarTest,
        term_reading::{TermReading, Test},
        terminal::Terminal,
        value_test::{ComplementScope, Polarity},
    },
    target_language::TargetLanguage,
    trailing_path::MemberPath,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use langtag::LangTagBuf;
use pretty_assertions::assert_eq;

/// The one term `q` lowers to under `matching`.
fn term_under(q: &str, matching: LanguageMatching) -> TermReading {
    match FilterReading::lowered(&parse(q).expect("a clause 4.9 filter"), matching) {
        FilterReading::Term(term) => *term,
        other @ (FilterReading::All(_) | FilterReading::Any(_)) => panic!("{q} lowers to {other:?}"),
    }
}

/// The one term `q` lowers to under the specification's tag equality.
fn term(q: &str) -> TermReading {
    term_under(q, LanguageMatching::TagEquality)
}

fn name(text: &str) -> AttributeName {
    text.parse().expect("an attribute name")
}

fn names(texts: &[&str]) -> Vec<AttributeName> {
    texts.iter().copied().map(name).collect()
}

fn members(texts: &[&str]) -> MemberPath {
    MemberPath::new(names(texts))
}

const OBSERVED_AT: &str = "\"https://uri.etsi.org/ngsi-ld/observedAt\"";

const UNIT_CODE: &str = "\"https://uri.etsi.org/ngsi-ld/unitCode\"";

/// LW-1.
#[test]
fn a_bare_attribute_reads_its_own_value_through_no_hop_and_no_sub_attribute() {
    let term = term("a==1");
    assert_eq!(term.hops(), Vec::<Hop>::new());
    assert_eq!(term.attribute(), &name("a"));
    assert_eq!(term.sub_attributes(), Vec::<AttributeName>::new());
    assert_eq!(term.terminal(), &Terminal::OwnValue);
}

/// LW-2. The Equal and Unequal semantics write `color[*]`, which reads every
/// value of the attribute, its own value.
#[test]
fn the_every_language_marker_reads_the_own_value() {
    assert_eq!(term("a[*]==1").terminal(), &Terminal::OwnValue);
}

/// LW-3.
#[test]
fn a_dotted_name_walks_one_sub_attribute() {
    let term = term("a.b==1");
    assert_eq!(term.sub_attributes(), names(&["b"]));
    assert_eq!(term.terminal(), &Terminal::OwnValue);
}

/// LW-4. `observedAt` is a non-reified Property of the instance before it.
#[test]
fn a_last_segment_naming_an_instance_member_ends_on_that_member() {
    let term = term(&format!("a.{OBSERVED_AT}>2024-01-01T00:00:00Z"));
    assert_eq!(term.sub_attributes(), Vec::<AttributeName>::new());
    assert_eq!(term.terminal(), &Terminal::InstanceMember(InstanceMetadata::ObservedAt));
}

/// LW-5.
#[test]
fn an_instance_member_below_a_sub_attribute_ends_on_the_sub_attribute_instance() {
    let term = term(&format!("a.b.{UNIT_CODE}==\"CEL\""));
    assert_eq!(term.sub_attributes(), names(&["b"]));
    assert_eq!(term.terminal(), &Terminal::InstanceMember(InstanceMetadata::UnitCode));
}

/// The every-language marker after an instance member reads the member, as it
/// reads an instance's own value.
#[test]
fn the_every_language_marker_after_an_instance_member_reads_the_member() {
    assert_eq!(
        term(&format!("a.{OBSERVED_AT}[*]>2024-01-01T00:00:00Z")).terminal(),
        &Terminal::InstanceMember(InstanceMetadata::ObservedAt)
    );
}

/// Clause 4.9 evaluates a trailing group "as a `MemberExpression`" against the
/// seed target value, and an instance member's value is a `DateTime` with no
/// member, so "the target element shall be considered as non-existent": the
/// member is no target, and the segment is a sub-attribute name no Attribute
/// carries (clause 4.6.2).
#[test]
fn an_instance_member_followed_by_a_trailing_group_is_no_member_target() {
    let term = term(&format!("a.{OBSERVED_AT}[x]==1"));
    assert_eq!(term.sub_attributes(), names(&["https://uri.etsi.org/ngsi-ld/observedAt"]));
    assert_eq!(term.terminal(), &Terminal::Members(members(&["x"])));
}

/// A non-reified Property has nothing beneath it, so a member named before the
/// last segment walks to no sub-attribute.
#[test]
fn an_instance_member_before_the_last_segment_is_a_sub_attribute_name() {
    let term = term(&format!("a.{OBSERVED_AT}.b==1"));
    assert_eq!(term.sub_attributes(), names(&["https://uri.etsi.org/ngsi-ld/observedAt", "b"]));
    assert_eq!(term.terminal(), &Terminal::OwnValue);
}

/// LW-6.
#[test]
fn a_one_member_group_spelled_like_a_tag_is_read_by_family() {
    assert_eq!(
        term("a[en]==\"x\"").terminal(),
        &Terminal::ByFamily {
            language: LangTagBuf::new("en".to_owned()).expect("a tag"),
            matching: LanguageMatching::TagEquality,
            members: members(&["en"]),
        }
    );
}

/// LW-7. Annex C.5.13.2 writes `marque[en-GB]`, which no `AttrName` spells.
#[test]
fn a_target_language_only_a_tag_spells_is_a_language_terminal() {
    assert_eq!(
        term("a[en-GB]==\"x\"").terminal(),
        &Terminal::Language {
            language: TargetLanguage::parse("en-GB").expect("a tag"),
            matching: LanguageMatching::TagEquality,
        }
    );
}

/// LW-8. `city_name` is no RFC 5646 tag, so the group reads a member only.
#[test]
fn a_one_member_group_that_is_no_tag_reads_the_compound_member() {
    assert_eq!(term("a[city_name]==\"x\"").terminal(), &Terminal::Members(members(&["city_name"])));
}

/// A four-letter member is an RFC 5646 `4ALPHA` language subtag, so it has the
/// language reading beside the member one.
#[test]
fn a_four_letter_member_is_also_a_language_tag() {
    assert!(matches!(term("a[city]==\"x\"").terminal(), Terminal::ByFamily { .. }));
}

/// LW-9.
#[test]
fn a_dotted_group_reads_a_member_path() {
    assert_eq!(term("a[b.c]==1").terminal(), &Terminal::Members(members(&["b", "c"])));
}

/// LW-10.
#[test]
fn a_hop_with_no_guard_admits_every_linked_entity() {
    let term = term("r{a}==1");
    let [hop] = term.hops() else {
        panic!("one hop: {:?}", term.hops());
    };
    assert_eq!(hop.relationship(), &name("r"));
    assert_eq!(hop.guard_types(), Vec::<EntityTypeName>::new());
    assert_eq!(term.attribute(), &name("a"));
}

/// LW-11.
#[test]
fn a_chain_of_two_hops_keeps_each_guard_outermost_first() {
    let term = term("r{T1,T2:s{a}}==1");
    let [outer, inner] = term.hops() else {
        panic!("two hops: {:?}", term.hops());
    };
    assert_eq!(outer.relationship(), &name("r"));
    assert_eq!(
        outer.guard_types(),
        ["T1", "T2"].map(|text| text.parse::<EntityTypeName>().expect("a type name"))
    );
    assert_eq!(inner.relationship(), &name("s"));
    assert_eq!(inner.guard_types(), Vec::<EntityTypeName>::new());
    assert_eq!(term.attribute(), &name("a"));
}

/// LW-13. Clause 4.9 EXAMPLE 14 writes a guard of one type, which the ABNF's
/// two or more types does not admit.
#[test]
fn a_hop_guarded_by_one_type_keeps_its_guard() {
    let term = term("r{T1:s{a}}==1");
    let [outer, inner] = term.hops() else {
        panic!("two hops: {:?}", term.hops());
    };
    assert_eq!(outer.relationship(), &name("r"));
    assert_eq!(outer.guard_types(), ["T1".parse::<EntityTypeName>().expect("a type name")]);
    assert_eq!(inner.relationship(), &name("s"));
    assert_eq!(inner.guard_types(), Vec::<EntityTypeName>::new());
    assert_eq!(term.attribute(), &name("a"));
}

/// LW-12. Clause 4.9 makes each name after the first "a (sub)Property or
/// (sub)Relationship", and a Property's value member is neither: a compound
/// value is entered through the trailing group. The segment is a sub-attribute
/// name, which no Attribute carries.
#[test]
fn a_segment_naming_a_family_value_member_is_a_sub_attribute_name() {
    let term = term("a.\"https://uri.etsi.org/ngsi-ld/hasValue\"==1");
    assert_eq!(term.sub_attributes(), names(&["https://uri.etsi.org/ngsi-ld/hasValue"]));
    assert_eq!(term.terminal(), &Terminal::OwnValue);
}

/// LW-15. The `language-ranges` level reads every target language by RFC 4647
/// extended filtering, and the reading carries it.
#[test]
fn a_target_language_carries_the_matching_it_was_lowered_under() {
    assert_eq!(
        term_under("a[en]==\"x\"", LanguageMatching::ExtendedFiltering).terminal(),
        &Terminal::ByFamily {
            language: LangTagBuf::new("en".to_owned()).expect("a tag"),
            matching: LanguageMatching::ExtendedFiltering,
            members: members(&["en"]),
        }
    );
}

/// `!=` is the complement of `==` over every kind, `!~=` of `~=` over
/// Strings, and an existence term tests nothing.
#[test]
fn a_negated_operator_lowers_to_the_complement_of_its_positive_test() {
    let Test::Value(unequal) = term("a!=1").test().clone() else {
        panic!("a value test");
    };
    assert_eq!(unequal.polarity(), Polarity::Complement(ComplementScope::AnyKind));
    assert_eq!(unequal.test(), &ScalarTest::EqualsAny(vec![Operand::Number("1".parse().expect("a number"))]));
    let Test::Value(unmatched) = term("a!~=\"x\"").test().clone() else {
        panic!("a value test");
    };
    assert_eq!(unmatched.polarity(), Polarity::Complement(ComplementScope::StringTarget));
    assert_eq!(term("a").test(), &Test::Exists);
}

/// The boolean structure is kept as written.
#[test]
fn a_filter_lowers_its_conjunctions_and_disjunctions_as_written() {
    let expr: Expr = "a==1;(b==2|c)".parse().expect("a filter");
    let FilterReading::All(children) = FilterReading::lowered(&expr, LanguageMatching::TagEquality) else {
        panic!("a conjunction");
    };
    assert!(matches!(children.as_slice(), [FilterReading::Term(_), FilterReading::Any(alternatives)] if alternatives.len() == 2));
}
