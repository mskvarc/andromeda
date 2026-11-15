//! Lowering a parsed `q` filter to its reading (clause 4.9).
//!
//! Outside the grammar, the serialiser and term expansion, this is the only
//! code that reads the variants of [`Predicate`], [`CompareValue`] and
//! [`Scalar`]: every clause 4.9 decision a Query Term needs is taken here
//! once, and the executions read the result.
//!
//! The path. A hop becomes a [`Hop`], outermost first. Clause 4.9 makes each
//! name after the first "a (sub)Property or (sub)Relationship", so every `.`
//! segment names a sub-attribute, except that a last one naming an instance
//! member is that member, unless a trailing group follows it: such a member's
//! value is a String or a `DateTime`, which has no member a `MemberExpression`
//! could reach, so "the target element shall be considered as non-existent".
//! A member named anywhere else is a sub-attribute name no Attribute carries,
//! since clause 4.6.2 bars the Core Context's non-reified Properties as
//! Attribute names, and a family's value member is no (sub)Property at all.
//! No trailing group and `[*]` read the instance's own value.
//!
//! The test. `!=` is the complement of `==` over every kind and `!~=` the
//! complement of `~=` over Strings, so no unequal comparison exists. A range
//! whose bounds are of two kinds holds for no target: clause 4.9 defines no
//! interval between values of two data types.

use crate::filter::{
    expression::Expr,
    language_matching::LanguageMatching,
    path::{AttrPath, QueryPath},
    predicate::{OrderingOp, Predicate, SetMembership},
    reading::{
        filter_reading::FilterReading,
        hop::Hop,
        interval::Interval,
        operand::Operand,
        order_relation::OrderRelation,
        scalar_test::ScalarTest,
        term_reading::{TermReading, Test},
        terminal::Terminal,
        value_test::{ComplementScope, Polarity, ValueTest},
    },
    scalar::{CompareValue, Scalar},
    target_language::LanguageSelector,
    trailing_path::TrailingPath,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

impl FilterReading {
    /// The reading of a parsed, expanded filter as written, term for term, its
    /// target languages selecting languageMap keys by `matching`: what the
    /// normal form is taken of.
    #[must_use]
    pub fn lowered(expr: &Expr, matching: LanguageMatching) -> FilterReading {
        match expr {
            Expr::And(terms) => FilterReading::All(terms.iter().map(|term| FilterReading::lowered(term, matching)).collect()),
            Expr::Or(terms) => FilterReading::Any(terms.iter().map(|term| FilterReading::lowered(term, matching)).collect()),
            Expr::Pred(predicate) => FilterReading::Term(Box::new(TermReading::of(predicate, matching))),
        }
    }
}

impl TermReading {
    /// The reading of one parsed, expanded Query Term, its target language
    /// selecting languageMap keys by `matching`.
    #[must_use]
    pub fn of(predicate: &Predicate, matching: LanguageMatching) -> TermReading {
        let (hops, local) = hops_to_local(predicate.path());
        let (sub_attributes, terminal) = local_reading(local, matching);
        TermReading {
            hops,
            attribute: local.attr.clone(),
            sub_attributes,
            terminal,
            test: test_of(predicate),
        }
    }
}

/// The hops of an attribute axis, outermost first, and the local path it ends
/// on.
fn hops_to_local(path: &QueryPath) -> (Vec<Hop>, &AttrPath) {
    let mut hops = Vec::new();
    let mut current = path;
    loop {
        match current {
            QueryPath::Local(local) => return (hops, local),
            QueryPath::Linked {
                relationship,
                guard_types,
                inner,
            } => {
                hops.push(Hop {
                    relationship: relationship.clone(),
                    guard_types: guard_types.iter().map(EntityTypeName::from).collect(),
                });
                current = inner;
            }
        }
    }
}

/// The sub-attributes a local path walks and what it ends on.
fn local_reading(path: &AttrPath, matching: LanguageMatching) -> (Vec<AttributeName>, Terminal) {
    match path.ending_member() {
        Some((member, above)) => (above.to_vec(), Terminal::InstanceMember(member)),
        None => (path.sub_attrs.clone(), trailing_terminal(path, matching)),
    }
}

/// What a path's trailing group ends on at the instance it reaches.
fn trailing_terminal(path: &AttrPath, matching: LanguageMatching) -> Terminal {
    match &path.trailing {
        None | Some(TrailingPath::AllLanguages) => Terminal::OwnValue,
        Some(TrailingPath::Language(language)) => Terminal::Language {
            language: language.clone(),
            matching,
        },
        Some(TrailingPath::Members(members)) => match path.target_language() {
            Some(LanguageSelector::Tag(tag)) => Terminal::ByFamily {
                language: tag.to_owned(),
                matching,
                members: members.clone(),
            },
            Some(LanguageSelector::Range(_)) | None => Terminal::Members(members.clone()),
        },
    }
}

/// What a Query Term asks of its target.
fn test_of(predicate: &Predicate) -> Test {
    match predicate {
        Predicate::Exists(_) => Test::Exists,
        Predicate::Equality { membership, rhs, .. } => Test::Value(ValueTest::new(equality_test(rhs), equality_polarity(*membership))),
        Predicate::Ordering { op, rhs, .. } => Test::Value(ValueTest::new(
            ScalarTest::Ordered {
                relation: order_relation(*op),
                operand: operand_of(rhs),
            },
            Polarity::Asserted,
        )),
        Predicate::MatchPattern { pattern, .. } => Test::Value(ValueTest::new(ScalarTest::Matches(pattern.clone()), Polarity::Asserted)),
        Predicate::NotMatchPattern { pattern, .. } => Test::Value(ValueTest::new(
            ScalarTest::Matches(pattern.clone()),
            Polarity::Complement(ComplementScope::StringTarget),
        )),
    }
}

/// The comparison `==` and `!=` make with a value, a value list or a range.
fn equality_test(rhs: &CompareValue) -> ScalarTest {
    match rhs {
        CompareValue::Scalar(scalar) => ScalarTest::EqualsAny(vec![operand_of(scalar)]),
        CompareValue::ValueList(values) => ScalarTest::EqualsAny(values.iter().map(operand_of).collect()),
        CompareValue::Range { min, max } => Interval::new(operand_of(min), operand_of(max)).map_or(ScalarTest::Unsatisfiable, ScalarTest::Within),
    }
}

/// `==` asserts its test and `!=` its complement over every kind.
const fn equality_polarity(membership: SetMembership) -> Polarity {
    match membership {
        SetMembership::Within => Polarity::Asserted,
        SetMembership::Outside => Polarity::Complement(ComplementScope::AnyKind),
    }
}

/// The relation an ordering operator tests.
const fn order_relation(op: OrderingOp) -> OrderRelation {
    match op {
        OrderingOp::Greater => OrderRelation::Greater,
        OrderingOp::GreaterEq => OrderRelation::GreaterOrEqual,
        OrderingOp::Less => OrderRelation::Less,
        OrderingOp::LessEq => OrderRelation::LessOrEqual,
    }
}

/// The operand a parsed scalar is: a bare `URI` compares as the text it is,
/// exactly as a `quotedStr` holding it.
fn operand_of(scalar: &Scalar) -> Operand {
    match scalar {
        Scalar::Number(number) => Operand::Number(number.clone()),
        Scalar::Bool(value) => Operand::Boolean(*value),
        Scalar::Str(text) | Scalar::Uri(text) => Operand::Text(text.clone()),
        Scalar::DateTime(instant) => Operand::DateTime(*instant),
        Scalar::Date(date) => Operand::Date(*date),
        Scalar::Time(time) => Operand::Time(*time),
    }
}
