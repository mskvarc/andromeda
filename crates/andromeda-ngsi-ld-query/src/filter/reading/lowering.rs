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
//! interval between values of two data types. A text operand of `==` or `!=`,
//! alone or in a value list, that RFC 3986 section 6 normalises to another
//! spelling is a [`UriOperand`]: the stored and forwarded filter keeps what the
//! client wrote, and the reading holds both spellings.

use crate::filter::{
    expression::Expr,
    language_matching::LanguageMatching,
    path::{AttrPath, QueryPath},
    predicate::{Predicate, SetMembership},
    reading::{
        filter_reading::FilterReading,
        hop::Hop,
        interval::Interval,
        operand::Operand,
        scalar_test::ScalarTest,
        term_reading::{TermReading, Test},
        terminal::Terminal,
        uri_operand::UriOperand,
        value_test::{Polarity, ValueTest},
    },
    scalar::{CompareValue, Scalar},
    target_language::LanguageSelector,
    trailing_path::TrailingPath,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};

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

    /// The reading of an Entity that carries at least one of `attributes`,
    /// which `attrs` asks for, "a synonym for a combination of the pick and q
    /// members" (clause 5.2.23): a disjunction of their existence terms, in
    /// normal form, so each name is one term and a single name is that term
    /// alone. With no name it is a disjunction no Entity satisfies.
    #[must_use]
    pub fn carrying_any(attributes: impl IntoIterator<Item = AttributeName>) -> FilterReading {
        FilterReading::Any(
            attributes
                .into_iter()
                .map(|attribute| {
                    FilterReading::Term(Box::new(TermReading {
                        hops: Vec::new(),
                        attribute,
                        sub_attributes: Vec::new(),
                        terminal: Terminal::OwnValue,
                        test: Test::Exists,
                    }))
                })
                .collect(),
        )
        .normal_form()
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
                relation: op.relation(),
                operand: Operand::from(rhs),
            },
            Polarity::Asserted,
        )),
        Predicate::MatchPattern { pattern, .. } => Test::Value(ValueTest::new(ScalarTest::Matches(pattern.clone()), Polarity::Asserted)),
        Predicate::NotMatchPattern { pattern, .. } => Test::Value(ValueTest::new(ScalarTest::Matches(pattern.clone()), Polarity::Complement)),
    }
}

/// The comparison `==` and `!=` make with a value, a value list or a range.
fn equality_test(rhs: &CompareValue) -> ScalarTest {
    match rhs {
        CompareValue::Scalar(scalar) => ScalarTest::EqualsAny(vec![equality_operand(scalar)]),
        CompareValue::ValueList(values) => ScalarTest::EqualsAny(values.iter().map(equality_operand).collect()),
        CompareValue::Range { min, max } => Interval::new(Operand::from(min), Operand::from(max)).map_or(ScalarTest::Unsatisfiable, ScalarTest::Within),
    }
}

/// The operand `==` and `!=` compare with `scalar`.
///
/// A text that parses as a URI the write path would store in another
/// spelling, the RFC 3986 section 6.2.2 and 6.2.3 normalisation `Identifier`
/// applies, compares in both: clause 4.9 asks that "URI comparison should be
/// performed so that the number of false negatives is minimized".
fn equality_operand(scalar: &Scalar) -> Operand {
    let (Scalar::Str(written) | Scalar::Uri(written)) = scalar else {
        return Operand::from(scalar);
    };
    match written.parse::<Identifier>() {
        Ok(parsed) if parsed.as_str() != written => Operand::Uri(UriOperand::new(written.clone(), parsed)),
        Ok(_) | Err(_) => Operand::from(scalar),
    }
}

/// `==` asserts its test and `!=` its complement.
const fn equality_polarity(membership: SetMembership) -> Polarity {
    match membership {
        SetMembership::Within => Polarity::Asserted,
        SetMembership::Outside => Polarity::Complement,
    }
}
