//! Lowering a parsed `q` filter to its reading (clause 4.9).
//!
//! Outside the grammar, the serialiser and term expansion, this is the only
//! code that reads the variants of [`QueryTerm`], [`EqualityValue`] and
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
//! since clause 4.5.1 bars the Core Context's non-reified Properties as
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
    Filter,
    attribute_path::AttributePath,
    equality_value::EqualityValue,
    instance_metadata::InstanceMetadata,
    language_matching::LanguageMatching,
    query_term::{QueryTerm, SetMembership},
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
    scalar::Scalar,
    target_language::LanguageSelector,
    trailing_group::TrailingGroup,
    value_path::ValuePath,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};

impl FilterReading {
    /// The reading of a parsed, expanded filter as written, term for term, its
    /// target languages selecting languageMap keys by `matching`: what the
    /// normal form is taken of.
    #[must_use]
    pub fn lowered(expr: &Filter, matching: LanguageMatching) -> FilterReading {
        match expr {
            Filter::And(terms) => FilterReading::All(terms.iter().map(|term| FilterReading::lowered(term, matching)).collect()),
            Filter::Or(terms) => FilterReading::Any(terms.iter().map(|term| FilterReading::lowered(term, matching)).collect()),
            Filter::Term(predicate) => FilterReading::Term(Box::new(TermReading::of(predicate, matching))),
        }
    }

    /// The reading of a parsed, expanded `csf` filter as written, term for
    /// term: what the normal form of a Context Source filter is taken of.
    ///
    /// A `csf` keeps the names below a Context Source Property as its client
    /// wrote them, so a last one naming an instance member does so by the core
    /// `@context` term clause 4.5.2.2 writes the member by (`tier.unitCode`),
    /// not by the IRI an Entity filter expands it to, and is that member: the
    /// core `@context` is protected, so the term names it under every
    /// `@context`. Before a trailing group it stays a sub-attribute name, as
    /// the IRI does.
    #[must_use]
    pub fn lowered_context_source_filter(expr: &Filter) -> FilterReading {
        FilterReading::lowered(expr, LanguageMatching::TagEquality).ending_on_member_terms()
    }

    /// The reading with every term whose last sub-attribute is an instance
    /// member's core `@context` term ending on that member.
    fn ending_on_member_terms(self) -> FilterReading {
        match self {
            FilterReading::All(children) => FilterReading::All(children.into_iter().map(FilterReading::ending_on_member_terms).collect()),
            FilterReading::Any(children) => FilterReading::Any(children.into_iter().map(FilterReading::ending_on_member_terms).collect()),
            FilterReading::Term(term) => FilterReading::Term(Box::new(term.ending_on_member_term())),
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
    pub fn of(predicate: &QueryTerm, matching: LanguageMatching) -> TermReading {
        let (hops, local) = hops_to_local(predicate.path());
        let (sub_attributes, terminal) = local_reading(local, matching);
        TermReading {
            hops,
            attribute: local.attribute.clone(),
            sub_attributes,
            terminal,
            test: test_of(predicate),
        }
    }

    /// The term ending on the instance member its last sub-attribute names by
    /// the member's core `@context` term, when the path ends there; the term
    /// as it is otherwise.
    fn ending_on_member_term(mut self) -> TermReading {
        if matches!(self.terminal, Terminal::OwnValue)
            && let Some(member) = self.sub_attributes.last().and_then(InstanceMetadata::termed)
        {
            self.sub_attributes.pop();
            self.terminal = Terminal::InstanceMember(member);
        }
        self
    }
}

/// The hops of an attribute axis, outermost first, and the local path it ends
/// on.
fn hops_to_local(path: &AttributePath) -> (Vec<Hop>, &ValuePath) {
    let mut hops = Vec::new();
    let mut current = path;
    loop {
        match current {
            AttributePath::Local(local) => return (hops, local),
            AttributePath::Linked {
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
fn local_reading(path: &ValuePath, matching: LanguageMatching) -> (Vec<AttributeName>, Terminal) {
    match path.ending_member() {
        Some((member, above)) => (above.to_vec(), Terminal::InstanceMember(member)),
        None => (path.sub_attributes.clone(), trailing_terminal(path, matching)),
    }
}

/// What a path's trailing group ends on at the instance it reaches.
fn trailing_terminal(path: &ValuePath, matching: LanguageMatching) -> Terminal {
    match &path.trailing {
        None | Some(TrailingGroup::AllLanguages) => Terminal::OwnValue,
        Some(TrailingGroup::Language(language)) => Terminal::Language {
            language: language.clone(),
            matching,
        },
        Some(TrailingGroup::Members(members)) => match path.target_language() {
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
fn test_of(predicate: &QueryTerm) -> Test {
    match predicate {
        QueryTerm::Exists(_) => Test::Exists,
        QueryTerm::Equality { membership, value: rhs, .. } => Test::Value(ValueTest::new(equality_test(rhs), equality_polarity(*membership))),
        QueryTerm::Ordering { operator: op, value: rhs, .. } => Test::Value(ValueTest::new(
            ScalarTest::Ordered {
                relation: op.relation(),
                operand: Operand::from(rhs),
            },
            Polarity::Asserted,
        )),
        QueryTerm::MatchPattern { pattern, .. } => Test::Value(ValueTest::new(ScalarTest::Matches(pattern.clone()), Polarity::Asserted)),
        QueryTerm::NotMatchPattern { pattern, .. } => Test::Value(ValueTest::new(ScalarTest::Matches(pattern.clone()), Polarity::Complement)),
    }
}

/// The comparison `==` and `!=` make with a value, a value list or a range.
fn equality_test(rhs: &EqualityValue) -> ScalarTest {
    match rhs {
        EqualityValue::Scalar(scalar) => ScalarTest::EqualsAny(vec![equality_operand(scalar)]),
        EqualityValue::ValueList(values) => ScalarTest::EqualsAny(values.iter().map(equality_operand).collect()),
        EqualityValue::Range { min, max } => Interval::new(Operand::from(min), Operand::from(max)).map_or(ScalarTest::Unsatisfiable, ScalarTest::Within),
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
