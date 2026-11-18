//! What one lowered Query Term makes of a target of each kind and of an array
//! of each kind, against clause 4.9.
//!
//! The snapshots print the whole table, operator group by operator group, and
//! mark each answer that rests on a recorded ruling rather than on a sentence
//! of the clause, so a change of meaning is one reviewed diff.

use crate::filter::{
    Filter,
    language_matching::LanguageMatching,
    reading::{
        array_rule::ArrayRule,
        array_target::ArrayTarget,
        filter_reading::FilterReading,
        interval::Interval,
        operand::Operand,
        order_relation::OrderRelation,
        scalar_test::ScalarTest,
        target_kind::TargetKind,
        term_reading::Test,
        value_test::{ComplementScope, Polarity, ValueTest},
        verdict::Verdict,
    },
};
use andromeda_architecture_checks::assert_named_snapshot;
use andromeda_ngsi_ld_lexicon::time::{
    date::NgsiLdDate,
    rendering::{instant_text, time_of_day_text},
};
use pretty_assertions::assert_eq;
use std::fmt::Write;
use strum::IntoEnumIterator;

/// The value test of the one term `q` lowers to.
fn value_test(q: &str) -> ValueTest {
    let filter = FilterReading::lowered(&q.parse::<Filter>().expect("a clause 4.9 filter"), LanguageMatching::TagEquality);
    let FilterReading::Term(term) = filter else {
        panic!("{q} lowers to {filter:?}");
    };
    match term.test() {
        Test::Value(test) => test.clone(),
        Test::Exists => panic!("{q} tests no value"),
    }
}

/// The verdict of every kind, in declaration order.
fn verdicts(test: &ValueTest) -> Vec<(TargetKind, Verdict<'_>)> {
    TargetKind::iter().map(|kind| (kind, test.verdict(kind))).collect()
}

/// The verdict of every kind: `on` for the listed kinds, `otherwise` for the
/// rest.
fn expected<'t>(on: &[(TargetKind, Verdict<'t>)], otherwise: Verdict<'t>) -> Vec<(TargetKind, Verdict<'t>)> {
    TargetKind::iter()
        .map(|kind| (kind, on.iter().find(|(listed, _)| *listed == kind).map_or(otherwise, |(_, verdict)| *verdict)))
        .collect()
}

/// The array rule of every array target, in declaration order.
fn array_rules(test: &ValueTest) -> Vec<ArrayRule> {
    ArrayTarget::iter().map(|target| test.array_rule(target)).collect()
}

fn number(text: &str) -> Operand {
    Operand::Number(text.parse().expect("a number"))
}

fn text(value: &str) -> Operand {
    Operand::Text(value.to_owned())
}

fn equals(operands: Vec<Operand>) -> ScalarTest {
    ScalarTest::EqualsAny(operands)
}

fn ordered(relation: OrderRelation, operand: Operand) -> ScalarTest {
    ScalarTest::Ordered { relation, operand }
}

fn within(min: Operand, max: Operand) -> ScalarTest {
    ScalarTest::Within(Interval::new(min, max).expect("bounds of one kind"))
}

const SOME_ELEMENT: [ArrayRule; 3] = [ArrayRule::SomeElement; 3];

const NO_ELEMENT: [ArrayRule; 3] = [ArrayRule::NoElement; 3];

/// The array rules of an ordering, a range or a pattern: a value or object
/// array is another data type, and any one string of a languageMap the
/// target value.
const ONE_OF_A_MAP: [ArrayRule; 3] = [ArrayRule::Never, ArrayRule::Never, ArrayRule::SomeElement];

/// Clause 4.9 Equal: "If there is no equality between the target value
/// data type and the Query Term value data type, then it shall be considered as
/// not matching"; an array includes the value.
#[test]
fn an_equality_with_a_number_holds_on_numbers_alone() {
    let test = value_test("x==5");
    let five = equals(vec![number("5")]);
    assert_eq!(verdicts(&test), expected(&[(TargetKind::Number, Verdict::Holds(&five))], Verdict::Never));
    assert_eq!(array_rules(&test), SOME_ELEMENT);
}

/// Clause 4.9 Unequal: "If the data type of the target value and the data
/// type of the Query Term value are different, then they shall be considered
/// unequal".
#[test]
fn an_inequality_with_a_number_holds_on_every_other_kind() {
    let test = value_test("x!=5");
    let five = equals(vec![number("5")]);
    assert_eq!(verdicts(&test), expected(&[(TargetKind::Number, Verdict::HoldsNot(&five))], Verdict::Always));
    assert_eq!(array_rules(&test), NO_ELEMENT);
}

/// The infinities order around every finite number, and
/// not-a-number is unordered, a recorded ruling.
#[test]
fn an_ordering_with_a_number_places_the_infinities_by_sign() {
    for (q, relation, positive_infinity) in [
        ("x>5", OrderRelation::Greater, Verdict::Always),
        ("x>=5", OrderRelation::GreaterOrEqual, Verdict::Always),
        ("x<5", OrderRelation::Less, Verdict::Never),
        ("x<=5", OrderRelation::LessOrEqual, Verdict::Never),
    ] {
        let test = value_test(q);
        let bound = ordered(relation, number("5"));
        let negative_infinity = match positive_infinity {
            Verdict::Always => Verdict::Never,
            Verdict::Never | Verdict::Holds(_) | Verdict::HoldsNot(_) => Verdict::Always,
        };
        assert_eq!(
            verdicts(&test),
            expected(
                &[
                    (TargetKind::Number, Verdict::Holds(&bound)),
                    (TargetKind::PositiveInfinity, positive_infinity),
                    (TargetKind::NegativeInfinity, negative_infinity),
                ],
                Verdict::Never
            ),
            "{q}"
        );
        assert_eq!(array_rules(&test), ONE_OF_A_MAP, "{q}");
    }
}

/// Clause 4.9 compares a String operand with a Relationship's
/// object under equality.
#[test]
fn an_equality_with_text_holds_on_strings_and_relationship_objects() {
    let a = equals(vec![text("a")]);
    assert_eq!(
        verdicts(&value_test("x==\"a\"")),
        expected(
            &[(TargetKind::String, Verdict::Holds(&a)), (TargetKind::RelationshipObject, Verdict::Holds(&a))],
            Verdict::Never
        )
    );
    assert_eq!(
        verdicts(&value_test("x!=\"a\"")),
        expected(
            &[
                (TargetKind::String, Verdict::HoldsNot(&a)),
                (TargetKind::RelationshipObject, Verdict::HoldsNot(&a))
            ],
            Verdict::Always
        )
    );
}

/// Clause 4.9 bars a Relationship from "any operator different than
/// equal or unequal".
#[test]
fn an_ordering_with_text_holds_on_strings_and_never_on_relationship_objects() {
    let a = ordered(OrderRelation::Greater, text("a"));
    assert_eq!(
        verdicts(&value_test("x>\"a\"")),
        expected(&[(TargetKind::String, Verdict::Holds(&a))], Verdict::Never)
    );
}

/// A bare `URI` compares as the text it is, exactly as its quoted
/// spelling.
#[test]
fn a_bare_uri_lowers_to_the_test_of_its_quoted_spelling() {
    assert_eq!(value_test("x==urn:a:b"), value_test("x==\"urn:a:b\""));
}

#[test]
fn an_equality_with_a_boolean_holds_on_booleans_alone() {
    let yes = equals(vec![Operand::Boolean(true)]);
    assert_eq!(
        verdicts(&value_test("x==true")),
        expected(&[(TargetKind::Boolean, Verdict::Holds(&yes))], Verdict::Never)
    );
    assert_eq!(
        verdicts(&value_test("x!=true")),
        expected(&[(TargetKind::Boolean, Verdict::HoldsNot(&yes))], Verdict::Always)
    );
}

/// A time operand matches only a value of its own clause
/// 4.6.3 form.
#[test]
fn a_time_operand_holds_on_its_own_form_alone() {
    for (q, kind) in [
        ("x==2024-01-15T00:00:00Z", TargetKind::DateTime),
        ("x==2024-01-15", TargetKind::Date),
        ("x==12:00:00Z", TargetKind::Time),
    ] {
        let test = value_test(q);
        let ScalarTest::EqualsAny(_) = test.test() else {
            panic!("{q} is an equality");
        };
        assert_eq!(verdicts(&test), expected(&[(kind, Verdict::Holds(test.test()))], Verdict::Never), "{q}");
    }
    let test = value_test("x!=2024-01-15");
    assert_eq!(
        verdicts(&test),
        expected(&[(TargetKind::Date, Verdict::HoldsNot(test.test()))], Verdict::Always)
    );
}

#[test]
fn a_number_range_holds_on_finite_numbers_alone() {
    let test = value_test("x==1..5");
    let range = within(number("1"), number("5"));
    assert_eq!(verdicts(&test), expected(&[(TargetKind::Number, Verdict::Holds(&range))], Verdict::Never));
    assert_eq!(array_rules(&test), ONE_OF_A_MAP);
}

/// A range is a value of the `unequal` production, so a Relationship's
/// object is unequal to it like any other kind.
#[test]
fn a_negated_number_range_holds_on_every_other_kind_relationship_objects_included() {
    let test = value_test("x!=1..5");
    let range = within(number("1"), number("5"));
    assert_eq!(verdicts(&test), expected(&[(TargetKind::Number, Verdict::HoldsNot(&range))], Verdict::Always));
    assert_eq!(array_rules(&test), [ArrayRule::Always, ArrayRule::Always, ArrayRule::NoElement]);
}

/// The bounds of a text range are ordered, which clause 4.9 bars a Relationship from.
#[test]
fn a_text_range_holds_on_strings_and_never_on_relationship_objects() {
    let range = within(text("a"), text("m"));
    assert_eq!(
        verdicts(&value_test("x==\"a\"..\"m\"")),
        expected(&[(TargetKind::String, Verdict::Holds(&range))], Verdict::Never)
    );
}

#[test]
fn a_number_list_holds_on_numbers_and_its_complement_on_every_other_kind() {
    let list = equals(vec![number("1"), number("2")]);
    let asserted = value_test("x==1,2");
    assert_eq!(verdicts(&asserted), expected(&[(TargetKind::Number, Verdict::Holds(&list))], Verdict::Never));
    assert_eq!(array_rules(&asserted), SOME_ELEMENT);
    let negated = value_test("x!=1,2");
    assert_eq!(verdicts(&negated), expected(&[(TargetKind::Number, Verdict::HoldsNot(&list))], Verdict::Always));
    assert_eq!(array_rules(&negated), NO_ELEMENT);
}

/// One test holds both operands, and each compares with its own kinds.
#[test]
fn a_mixed_list_holds_each_operand_against_its_own_kinds() {
    let list = equals(vec![text("a"), number("5")]);
    assert_eq!(
        verdicts(&value_test("x==\"a\",5")),
        expected(
            &[
                (TargetKind::Number, Verdict::Holds(&list)),
                (TargetKind::String, Verdict::Holds(&list)),
                (TargetKind::RelationshipObject, Verdict::Holds(&list)),
            ],
            Verdict::Never
        )
    );
}

/// Clause 4.9 applies both pattern operators to a String
/// target only: "If the target value data type is different than String then
/// it shall be considered as not matching."
#[test]
fn a_pattern_and_its_negation_hold_on_strings_alone() {
    let asserted = value_test("x~=\"a.*\"");
    let pattern = asserted.test().clone();
    assert_eq!(verdicts(&asserted), expected(&[(TargetKind::String, Verdict::Holds(&pattern))], Verdict::Never));
    assert_eq!(array_rules(&asserted), ONE_OF_A_MAP);
    let negated = value_test("x!~=\"a.*\"");
    assert_eq!(negated.polarity(), Polarity::Complement);
    assert_eq!(
        verdicts(&negated),
        expected(&[(TargetKind::String, Verdict::HoldsNot(&pattern))], Verdict::Never)
    );
    assert_eq!(array_rules(&negated), [ArrayRule::Never, ArrayRule::Never, ArrayRule::NoElement]);
}

/// Clause 4.9 Existence: "A matching entity shall contain the target
/// element."
#[test]
fn an_attribute_alone_tests_existence() {
    let filter = FilterReading::lowered(&"x".parse::<Filter>().expect("a filter"), LanguageMatching::TagEquality);
    let FilterReading::Term(term) = filter else {
        panic!("one term");
    };
    assert_eq!(term.test(), &Test::Exists);
}

/// A recorded ruling extends "No matching value is found" to a range.
#[test]
fn a_negated_text_range_holds_on_a_map_with_no_value_inside_it() {
    let test = value_test("x!=\"a\"..\"m\"");
    let range = within(text("a"), text("m"));
    assert_eq!(test.verdict(TargetKind::String), Verdict::HoldsNot(&range));
    assert_eq!(test.array_rule(ArrayTarget::AnyOneValue), ArrayRule::NoElement);
}

/// A recorded ruling extends "No matching value is found" to `!~=`.
#[test]
fn a_negated_pattern_holds_on_a_map_with_no_value_matching_it() {
    let test = value_test("x!~=\"a\"");
    assert_eq!(test.verdict(TargetKind::String), Verdict::HoldsNot(test.test()));
    assert_eq!(test.array_rule(ArrayTarget::AnyOneValue), ArrayRule::NoElement);
}

/// Clause 4.9 defines no interval between values of two data
/// types, a recorded ruling.
#[test]
fn a_range_of_two_kinds_holds_for_no_target_and_its_complement_for_every_one() {
    let asserted = value_test("x==1..\"z\"");
    assert_eq!(asserted.test(), &ScalarTest::Unsatisfiable);
    assert_eq!(verdicts(&asserted), expected(&[], Verdict::Never));
    assert_eq!(array_rules(&asserted), ONE_OF_A_MAP);
    let negated = value_test("x!=1..\"z\"");
    assert_eq!(verdicts(&negated), expected(&[], Verdict::Always));
    assert_eq!(array_rules(&negated), [ArrayRule::Always, ArrayRule::Always, ArrayRule::NoElement]);
}

/// "No element satisfies the asserted test": an element of an array target is
/// tested with the asserted test under either polarity, so a negated equality
/// tests each element for equality and a negated range finds no element inside
/// it of an array that is no target of a range.
#[test]
fn an_element_is_tested_with_the_asserted_test_whatever_the_polarity() {
    let negated = value_test("x!=1");
    let ones = equals(vec![number("1")]);
    assert_eq!(negated.verdict(TargetKind::Number), Verdict::HoldsNot(&ones));
    assert_eq!(negated.asserted_verdict(TargetKind::Number), Verdict::Holds(&ones));
    assert_eq!(negated.asserted_verdict(TargetKind::String), Verdict::Never);
    assert_eq!(negated.asserted_array_rule(ArrayTarget::ValueArray), ArrayRule::SomeElement);
    let negated_range = value_test("x!=1..5");
    assert_eq!(negated_range.array_rule(ArrayTarget::ValueArray), ArrayRule::Always);
    assert_eq!(negated_range.asserted_array_rule(ArrayTarget::ValueArray), ArrayRule::Never);
    assert_eq!(negated_range.asserted_array_rule(ArrayTarget::AnyOneValue), ArrayRule::SomeElement);
}

/// Clause 4.9 Unequal makes a target of another data type unequal, and Do
/// not match pattern makes it not matching, so `!=` complements over every
/// kind and `!~=` over Strings; an asserted test has no complement.
#[test]
fn the_scope_of_a_complement_follows_from_its_test() {
    for (q, scope) in [
        ("x!=1", Some(ComplementScope::AnyKind)),
        ("x!=1,2", Some(ComplementScope::AnyKind)),
        ("x!=1..5", Some(ComplementScope::AnyKind)),
        ("x!=1..\"z\"", Some(ComplementScope::AnyKind)),
        ("x!~=\"a.*\"", Some(ComplementScope::StringTarget)),
        ("x==1", None),
        ("x>1", None),
        ("x~=\"a.*\"", None),
    ] {
        assert_eq!(value_test(q).complement_scope(), scope, "{q}");
    }
}

/// A recorded ruling an answer of the table rests on, or `None` when a
/// sentence of clause 4.9 states it.
fn ruling_behind(test: &ValueTest, cell: Cell) -> Option<&'static str> {
    if matches!(test.test(), ScalarTest::Unsatisfiable) {
        return Some("a range of two kinds holds for no target");
    }
    match cell {
        Cell::Kind(kind) if kind.non_finite().is_some() && test.test().compares_with(kind) => Some("numbers beyond the decimals follow XSD"),
        Cell::Array(ArrayTarget::AnyOneValue)
            if matches!(test.polarity(), Polarity::Complement) && matches!(test.test(), ScalarTest::Within(_) | ScalarTest::Matches(_)) =>
        {
            Some("no value of a languageMap satisfies the asserted test")
        }
        Cell::Kind(_) | Cell::Array(_) => None,
    }
}

/// One column of the table.
#[derive(Debug, Clone, Copy)]
enum Cell {
    /// A target of one kind.
    Kind(TargetKind),
    /// A target holding several values.
    Array(ArrayTarget),
}

fn spelled_operand(operand: &Operand) -> String {
    match operand {
        Operand::Number(number) => number.to_string(),
        Operand::Text(text) => format!("{text:?}"),
        Operand::Uri(uri) => format!("{:?} or the object {:?}", uri.written(), uri.against(TargetKind::RelationshipObject)),
        Operand::Boolean(value) => value.to_string(),
        Operand::DateTime(instant) => instant_text(*instant),
        Operand::Date(date) => NgsiLdDate::from(*date).to_string(),
        Operand::Time(time) => time_of_day_text(*time),
    }
}

fn spelled_test(test: &ScalarTest) -> String {
    match test {
        ScalarTest::EqualsAny(operands) => {
            format!("equals {}", operands.iter().map(spelled_operand).collect::<Vec<_>>().join(" or "))
        }
        ScalarTest::Ordered { relation, operand } => {
            let token = match relation {
                OrderRelation::Greater => ">",
                OrderRelation::GreaterOrEqual => ">=",
                OrderRelation::Less => "<",
                OrderRelation::LessOrEqual => "<=",
            };
            format!("{token} {}", spelled_operand(operand))
        }
        ScalarTest::Within(interval) => format!("within {}..{}", spelled_operand(interval.min()), spelled_operand(interval.max())),
        ScalarTest::Matches(pattern) => format!("matches {:?}", pattern.as_str()),
        ScalarTest::Unsatisfiable => "unsatisfiable".to_owned(),
    }
}

fn spelled_verdict(verdict: Verdict<'_>) -> String {
    match verdict {
        Verdict::Never => "Never".to_owned(),
        Verdict::Always => "Always".to_owned(),
        Verdict::Holds(test) => format!("Holds({})", spelled_test(test)),
        Verdict::HoldsNot(test) => format!("HoldsNot({})", spelled_test(test)),
    }
}

/// The table of every term of one operator group.
fn table(terms: &[&str]) -> String {
    let mut out = String::new();
    for q in terms {
        let test = value_test(q);
        writeln!(out, "{q}").expect("a string takes every write");
        let kinds = TargetKind::iter().map(|kind| (format!("{kind:?}"), spelled_verdict(test.verdict(kind)), Cell::Kind(kind)));
        let arrays = ArrayTarget::iter().map(|target| (format!("{target:?}"), format!("{:?}", test.array_rule(target)), Cell::Array(target)));
        for (column, answer, cell) in kinds.chain(arrays) {
            let line = match ruling_behind(&test, cell) {
                Some(ruling) => format!("  {column:<20} {answer:<34} ruling: {ruling}"),
                None => format!("  {column:<20} {answer}"),
            };
            writeln!(out, "{}", line.trim_end()).expect("a string takes every write");
        }
        out.push('\n');
    }
    out
}

#[test]
fn the_equality_operators_decide_every_kind_as_the_table_prints() {
    assert_named_snapshot!(
        "decision-table-equality",
        table(&[
            "x==5",
            "x!=5",
            "x==\"a\"",
            "x!=\"a\"",
            "x==true",
            "x!=true",
            "x==2024-01-15T00:00:00Z",
            "x!=2024-01-15T00:00:00Z",
            "x==2024-01-15",
            "x!=2024-01-15",
            "x==12:00:00Z",
            "x!=12:00:00Z",
        ])
    );
}

#[test]
fn the_ordering_operators_decide_every_kind_as_the_table_prints() {
    assert_named_snapshot!(
        "decision-table-ordering",
        table(&[
            "x>5",
            "x>=5",
            "x<5",
            "x<=5",
            "x>\"a\"",
            "x<=\"a\"",
            "x>2024-01-15T00:00:00Z",
            "x<2024-01-15",
            "x>=12:00:00Z",
        ])
    );
}

#[test]
fn the_ranges_decide_every_kind_as_the_table_prints() {
    assert_named_snapshot!(
        "decision-table-range",
        table(&[
            "x==1..5",
            "x!=1..5",
            "x==\"a\"..\"m\"",
            "x!=\"a\"..\"m\"",
            "x==2024-01-01..2024-12-31",
            "x!=2024-01-01..2024-12-31",
            "x==1..\"z\"",
            "x!=1..\"z\"",
        ])
    );
}

#[test]
fn the_value_lists_decide_every_kind_as_the_table_prints() {
    assert_named_snapshot!(
        "decision-table-list",
        table(&["x==1,2", "x!=1,2", "x==\"a\",5", "x!=\"a\",5", "x==\"a\",\"b\"", "x!=true,false"])
    );
}

#[test]
fn the_pattern_operators_decide_every_kind_as_the_table_prints() {
    assert_named_snapshot!("decision-table-pattern", table(&["x~=\"a.*\"", "x!~=\"a.*\""]));
}
