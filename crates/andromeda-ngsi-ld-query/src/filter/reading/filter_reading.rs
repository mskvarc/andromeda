//! A `q` filter with every clause 4.9 decision taken, and its normal form.
//!
//! One filter has many spellings: `b==2;a==1` and `a==1;b==2`, `a==1|a==2`
//! and `a==1,2`, a group inside a group of the same connective. The normal
//! form is the one reading they share, so what memoises a term, keys a
//! statement by its text or digests a query sees one filter where clients
//! wrote several. Every rewrite keeps what each term means. A term is
//! existential over the instances of its attribute, and two terms of one
//! conjunction may be satisfied by two different instances, so two terms
//! become one only where their disjunction is that one term.

use crate::filter::{expression::Expr, language_matching::LanguageMatching, reading::term_reading::TermReading};
use std::ops::ControlFlow;

/// The boolean structure of a lowered `q` filter over its term readings.
///
/// The order is canonical, the one the normal form puts the children of a
/// group in: groups before terms, terms by path and then by test, operands by
/// their value and patterns by their source text. It says nothing of what a
/// filter selects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterReading {
    /// Every child holds (`;`).
    All(Vec<FilterReading>),
    /// Some child holds (`|`).
    Any(Vec<FilterReading>),
    /// One Query Term.
    Term(Box<TermReading>),
}

impl FilterReading {
    /// The reading the executions run: `expr`, parsed and expanded, lowered
    /// with its target languages selecting languageMap keys by `matching`,
    /// and in normal form.
    #[must_use]
    pub fn of(expr: &Expr, matching: LanguageMatching) -> FilterReading {
        FilterReading::lowered(expr, matching).normal_form()
    }

    /// The reading the executions run of a `csf` filter, parsed and
    /// expanded, which reads a registration's Context Source Properties.
    ///
    /// Clause 5.2.9 makes those Properties, never a `LanguageProperty`, so no
    /// target language selects a string of theirs and the reading takes tag
    /// equality whatever the language-targeting level.
    #[must_use]
    pub fn of_context_source_filter(expr: &Expr) -> FilterReading {
        FilterReading::of(expr, LanguageMatching::TagEquality)
    }

    /// The reading in normal form, which holds for exactly the Entities this
    /// one holds for.
    ///
    /// A group inside a group of the same connective is spliced into it, and
    /// a group of one child is that child. A disjunction joins the terms that
    /// assert an equality over the same targets into one value list. The
    /// children of every group are in canonical order and each is there once,
    /// as is each operand of a value list.
    #[must_use]
    pub fn normal_form(self) -> FilterReading {
        match self {
            FilterReading::All(children) => Connective::All.normal_form(children),
            FilterReading::Any(children) => Connective::Any.normal_form(children),
            FilterReading::Term(term) => FilterReading::Term(Box::new(term.normal_form())),
        }
    }

    /// Every term reading, in pre-order.
    #[must_use]
    pub fn terms(&self) -> Vec<&TermReading> {
        match self {
            FilterReading::All(children) | FilterReading::Any(children) => children.iter().flat_map(FilterReading::terms).collect(),
            FilterReading::Term(term) => vec![term],
        }
    }

    /// Joins `term` into this reading when it is a term asserting an equality
    /// over the same targets, and gives `term` back otherwise.
    fn join_equality(&mut self, term: Box<TermReading>) -> Result<(), Box<TermReading>> {
        match self {
            FilterReading::Term(earlier) => earlier.join_equality(term),
            FilterReading::All(_) | FilterReading::Any(_) => Err(term),
        }
    }
}

/// The connective of a group.
#[derive(Debug, Clone, Copy)]
enum Connective {
    /// Every child holds.
    All,
    /// Some child holds.
    Any,
}

impl Connective {
    /// The normal form of a group of this connective over `children`.
    fn normal_form(self, children: Vec<FilterReading>) -> FilterReading {
        let mut members = Vec::with_capacity(children.len());
        for child in children {
            match (self, child.normal_form()) {
                (Connective::All, FilterReading::All(spliced)) | (Connective::Any, FilterReading::Any(spliced)) => members.extend(spliced),
                (Connective::All, member @ (FilterReading::Any(_) | FilterReading::Term(_)))
                | (Connective::Any, member @ (FilterReading::All(_) | FilterReading::Term(_))) => members.push(member),
            }
        }
        let mut members = match self {
            Connective::All => members,
            Connective::Any => joined_equalities(members),
        };
        members.sort();
        members.dedup();
        match <[FilterReading; 1]>::try_from(members) {
            Ok([only]) => only,
            Err(members) => match self {
                Connective::All => FilterReading::All(members),
                Connective::Any => FilterReading::Any(members),
            },
        }
    }
}

/// The members of a disjunction, each term asserting an equality joined into
/// the first one before it over the same targets.
///
/// `a==1|a==2` holds when some target equals 1 or some target equals 2, which
/// is when some target equals one of the two, so the disjunction of two value
/// lists over the same targets is their union. A complement is never joined:
/// `a!=1|a!=2` holds for every present `a`, and `a!=1,2` does not.
fn joined_equalities(members: Vec<FilterReading>) -> Vec<FilterReading> {
    let mut joined: Vec<FilterReading> = Vec::with_capacity(members.len());
    for member in members {
        match member {
            FilterReading::Term(term) => {
                let unjoined = joined.iter_mut().try_fold(term, |term, earlier| match earlier.join_equality(term) {
                    Ok(()) => ControlFlow::Break(()),
                    Err(term) => ControlFlow::Continue(term),
                });
                if let ControlFlow::Continue(term) = unjoined {
                    joined.push(FilterReading::Term(term));
                }
            }
            group @ (FilterReading::All(_) | FilterReading::Any(_)) => joined.push(group),
        }
    }
    joined
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expression::Expr,
        language_matching::LanguageMatching,
        parser::parse,
        path::QueryPath,
        reading::filter_reading::FilterReading,
        trailing_path::{MemberPath, TrailingPath},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;

    /// The reading the executions run for `q`.
    fn normal(q: &str) -> FilterReading {
        FilterReading::of(&parse(q).expect("a clause 4.9 filter"), LanguageMatching::TagEquality)
    }

    /// `q` lowered term for term, before normal form.
    fn lowered(q: &str) -> FilterReading {
        FilterReading::lowered(&parse(q).expect("a clause 4.9 filter"), LanguageMatching::TagEquality)
    }

    /// A conjunction inside a conjunction is spliced into it.
    #[test]
    fn a_conjunction_inside_a_conjunction_is_one_conjunction() {
        let reading = normal("a==1;(b==2;c==3)");
        assert!(matches!(&reading, FilterReading::All(children) if children.len() == 3), "{reading:?}");
        assert_eq!(reading, lowered("a==1;b==2;c==3"));
    }

    #[test]
    fn a_term_written_twice_is_one_term() {
        assert_eq!(normal("a==1;a==1"), lowered("a==1"));
        assert_eq!(normal("a==1|a==1"), lowered("a==1"));
        assert_eq!(normal("a;a"), lowered("a"));
    }

    #[test]
    fn the_children_of_a_group_are_in_canonical_order() {
        assert_eq!(normal("b==2;a==1"), normal("a==1;b==2"));
        assert_eq!(normal("b==2;a==1"), lowered("a==1;b==2"));
        assert_eq!(normal("(c|b);a"), normal("a;(b|c)"));
    }

    #[test]
    fn a_disjunction_of_equalities_over_one_path_is_one_value_list() {
        let reading = normal("a==1|a==2");
        assert!(matches!(&reading, FilterReading::Term(_)), "{reading:?}");
        assert_eq!(reading, lowered("a==1,2"));
        assert_eq!(normal("a==2,3|a==1"), lowered("a==1,2,3"));
        assert_eq!(normal("a==\"x\"|a==5"), lowered("a==5,\"x\""));
    }

    /// `a!=1|a!=2` holds for every present `a`, and `a!=1,2` does not.
    #[test]
    fn complements_are_never_joined() {
        let reading = normal("a!=1|a!=2");
        assert!(matches!(&reading, FilterReading::Any(children) if children.len() == 2), "{reading:?}");
        assert_eq!(reading, lowered("a!=1|a!=2"));
        assert_eq!(normal("a==1|a!=2"), lowered("a==1|a!=2"));
    }

    /// Each term is its own existential over the instances, so an
    /// Entity whose instances are 3 and 12 satisfies the conjunction and not
    /// the range.
    #[test]
    fn two_bounds_are_never_one_range() {
        let reading = normal("a>=5;a<=10");
        assert!(matches!(&reading, FilterReading::All(children) if children.len() == 2), "{reading:?}");
        assert_eq!(reading, lowered("a>=5;a<=10"));
    }

    /// `1.0` and `1` are one value.
    #[test]
    fn operands_are_compared_by_value() {
        assert_eq!(normal("a==1.0|a==1"), lowered("a==1"));
        assert_eq!(normal("a==1e0;a==1"), lowered("a==1"));
        assert_eq!(normal("a==2,1.0,1"), lowered("a==1,2"));
    }

    /// Two equalities of one conjunction may be satisfied by two different
    /// instances, which one value list cannot say.
    #[test]
    fn equalities_of_a_conjunction_are_never_joined() {
        let reading = normal("a==1;a==2");
        assert!(matches!(&reading, FilterReading::All(children) if children.len() == 2), "{reading:?}");
    }

    /// A join needs the same hops, attribute, sub-attributes and terminal.
    #[test]
    fn equalities_over_different_targets_are_never_joined() {
        for q in [
            "a==1|a.b==2",
            "a==1|a[city_name]==2",
            "a[city_name]==1|a[zip_code]==2",
            "a.observedAt==2024-01-15T00:00:00Z|a.b==2024-01-15T00:00:00Z",
            "r{a}==1|s{a}==2",
            "r{T1,T2:a}==1|r{a}==2",
            "a==1|b==1",
            "a|a==1",
            "a==1|a>1",
            "a==1|a==1..5",
            "a==\"x\"|a~=\"x\"",
        ] {
            let reading = normal(q);
            assert!(
                matches!(&reading, FilterReading::Any(children) if children.len() == 2),
                "{q} normalises to {reading:?}"
            );
        }
        assert_eq!(normal("r{a}==1|r{a}==2"), lowered("r{a}==1,2"));
        assert_eq!(normal("a.b==1|a.b==2"), lowered("a.b==1,2"));
        assert_eq!(normal("a[city_name]==1|a[city_name]==2"), lowered("a[city_name]==1,2"));
    }

    /// A structured value is read by the addressed members and a raw JSON
    /// object by the written ones, so two written spellings of one addressed
    /// member read two keys of a `JsonProperty`.
    #[test]
    fn equalities_over_two_written_spellings_of_one_member_are_never_joined() {
        let mut expr = parse("a[city_name]==1|a[city_name]==2").expect("a filter");
        let Expr::Or(terms) = &mut expr else {
            panic!("a disjunction");
        };
        let Some(Expr::Pred(second)) = terms.get_mut(1) else {
            panic!("a second term");
        };
        let QueryPath::Local(local) = second.path_mut() else {
            panic!("a local path");
        };
        let member: AttributeName = "city_name".parse().expect("an attribute name");
        local.trailing = Some(TrailingPath::Members(MemberPath {
            addressed: vec![member],
            written: vec!["cityName".parse().expect("an attribute name")],
        }));
        let reading = FilterReading::of(&expr, LanguageMatching::TagEquality);
        assert!(matches!(&reading, FilterReading::Any(children) if children.len() == 2), "{reading:?}");
    }

    /// Splicing reaches the terms a nested group of the same connective
    /// holds, so they join as written side by side.
    #[test]
    fn a_disjunction_inside_a_disjunction_joins_its_equalities() {
        assert_eq!(normal("a==1|(b|a==2)"), lowered("a==1,2|b"));
        assert_eq!(normal("a==1|(c;d)|(b|a==2)"), normal("(c;d)|b|a==1,2"));
    }

    /// A group left with one child is that child, which then splices into
    /// its parent.
    #[test]
    fn a_group_of_one_child_is_that_child() {
        assert_eq!(normal("a;(b|b)"), lowered("a;b"));
        assert_eq!(normal("a|((b;c)|(b;c))"), lowered("(b;c)|a"));
        assert_eq!(normal("(a==1|a==2);b"), lowered("a==1,2;b"));
    }

    /// The normal form of a normal form is itself.
    #[test]
    fn the_normal_form_is_its_own_normal_form() {
        for q in [
            "a==1;(b==2;c==3)",
            "b==2|a==1|(c;a==5)",
            "a==1|(b|a==2)|a!=3",
            "(a|b);(b|a);c>1",
            "r{T1,T2:a}==1|r{T1,T2:a}==\"x\",2",
        ] {
            let reading = normal(q);
            assert_eq!(reading.clone().normal_form(), reading, "{q}");
        }
    }

    /// Lowering keeps the boolean structure as written, which the normal form
    /// is taken of.
    #[test]
    fn lowering_alone_keeps_the_filter_as_written() {
        let expr: Expr = "b==2;(a==1;a==1)".parse().expect("a filter");
        let FilterReading::All(children) = FilterReading::lowered(&expr, LanguageMatching::TagEquality) else {
            panic!("a conjunction");
        };
        assert!(matches!(children.as_slice(), [FilterReading::Term(_), FilterReading::All(inner)] if inner.len() == 2));
    }
}
