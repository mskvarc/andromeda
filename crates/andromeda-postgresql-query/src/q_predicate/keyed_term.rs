//! The one Query Term of a `q` expression an index answers on the page's
//! behalf, and the candidates the index reads for it.
//!
//! Five terms qualify: an equality on a sub-attribute's own value, read
//! through `idx_ca_sub_eq_keys`
//! ([`keyed_equality`](crate::q_predicate::sub_attribute::keyed_equality)),
//! a `~=` on an attribute's own value, read through the trigram indexes
//! ([`keyed_pattern`](crate::q_predicate::keyed_pattern)), an ordering or
//! a range on an attribute's own value with `DateTime` or `Date` operands,
//! read through the typed value indexes
//! ([`keyed_ordering`](crate::q_predicate::keyed_ordering)), and the bare
//! existence of an attribute, read through the GIN over the search row's
//! attribute names ([`keyed_existence`](crate::q_predicate::keyed_existence)),
//! and a Linked Entity hop, read inward from the linked entities its inner
//! term keeps ([`keyed_hop`](crate::q_predicate::keyed_hop)). Each candidate
//! is rechecked by the condition the term compiles to, or is a match as it is
//! read, so the candidates are exactly the Entities the term keeps.
//!
//! The candidate statement is unbounded. The planner cannot price any of the
//! terms (it underestimates a broad one by orders of magnitude), and a
//! statement whose matches are many is answered sooner by walking the
//! selection than by fetching every match through the index, so each
//! statement that reads the candidates bounds them by the most it can use and
//! walks past that bound. How far that is depends on what one candidate costs
//! to read ([`CandidateRead`]).

use crate::{
    entity_row_refs::EntityRowRefs,
    one_time_gate::CandidateBound,
    q_predicate::{
        keyed_existence::existence_candidates,
        keyed_hop::hop_candidates,
        keyed_ordering::ordering_candidates,
        keyed_pattern::pattern_candidates,
        resolved_filter::{ResolvedFilter, ResolvedTerm},
        sub_attribute::keyed_equality::equality_candidates,
    },
    query_names::QueryNames,
};
use andromeda_ngsi_ld_query::filter::reading::{
    filter_reading::FilterReading,
    scalar_test::ScalarTest,
    term_reading::{TermReading, Test},
    terminal::Terminal,
    value_test::Polarity,
};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::SelectStatement;
use std::slice::from_ref;

/// How the index path reads one candidate, which sets how many it reads
/// before the page walks instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRead {
    /// The index offers rows that may match and the term's condition rechecks
    /// each against its value: a trigram or a sub-attribute key match.
    Rechecked,
    /// The candidates are a range of a B-tree over the compared value, each
    /// a match as the index yields it.
    Ranged,
    /// The index offers the Entities whose search row names the attribute,
    /// and one probe of the grain constraint's leading columns confirms each.
    Probed,
    /// The inner term's index offers the linked entities it keeps, and an
    /// index over the object reaches each instance naming one, a match as it
    /// is read.
    Linked,
}

/// One term of a `q` filter's top-level conjunction and the candidates the
/// index path reads for it.
#[derive(Debug)]
pub struct KeyedCandidates<'q> {
    /// The term the candidates answer, which the page leaves out of the
    /// filter it applies to them.
    pub term: &'q TermReading,
    /// `SELECT "entity_key" …`, every candidate.
    pub select: SelectStatement,
    /// How one candidate is read.
    pub read: CandidateRead,
}

/// The term of the top-level conjunction of `reading` the index path answers
/// for the page, with its candidates read in the branch `rows` names, read as
/// far as `bound` admits for how they are read.
///
/// Only a conjunct can narrow the page to its candidates: an entity a
/// disjunction keeps for another arm need not be one of them. A filter no
/// Entity can match, its names resolved through `names`, offers none.
///
/// Of several conjuncts an index answers, the one taken is one whose census
/// count (`names.carriers`) fits its bound, the smallest such count first:
/// its candidates are Entities holding an instance of the attribute it is read
/// through, so the index answers it whole and the page never walks. When no
/// count fits, or none was read, the first in canonical order is taken: the
/// census bounds a value test's candidates and says nothing of how few it
/// keeps.
#[must_use]
pub fn keyed_candidates<'q>(
    reading: &'q FilterReading,
    rows: &EntityRowRefs,
    names: &QueryNames,
    bound: impl Fn(CandidateRead) -> CandidateBound,
) -> Option<KeyedCandidates<'q>> {
    let mut keyable = keyable_conjuncts(reading, rows, names);
    let fitting = keyable
        .iter()
        .enumerate()
        .filter_map(|(position, (keyed, carrier))| {
            let instances = names.carriers.instances(*carrier)?;
            bound(keyed.read).admits(instances).then_some((instances, position))
        })
        .min()
        .map_or(0, |(_, position)| position);
    (fitting < keyable.len()).then(|| keyable.swap_remove(fitting).0)
}

/// The attributes the census is read for before `reading` is rendered: the
/// attribute each conjunct an index answers is read through, when two or more
/// contend, and none otherwise, since one conjunct is taken without a count.
#[must_use]
pub fn contended_carriers(reading: &FilterReading, names: &QueryNames) -> Vec<AttributeNameKey> {
    let keyable = keyable_conjuncts(reading, &EntityRowRefs::unaliased(None), names);
    if keyable.len() < 2 {
        return Vec::new();
    }
    let mut carriers: Vec<AttributeNameKey> = keyable.into_iter().map(|(_, carrier)| carrier).collect();
    carriers.sort_unstable_by_key(|carrier| carrier.get());
    carriers.dedup();
    carriers
}

/// Every term of the top-level conjunction of `reading` an index answers, in
/// canonical order, each with the attribute its candidates are read through.
fn keyable_conjuncts<'q>(reading: &'q FilterReading, rows: &EntityRowRefs, names: &QueryNames) -> Vec<(KeyedCandidates<'q>, AttributeNameKey)> {
    let Some(resolved) = ResolvedFilter::of(reading, names) else {
        return Vec::new();
    };
    let conjuncts = match &resolved {
        ResolvedFilter::All(children) => children.as_slice(),
        ResolvedFilter::Any(_) | ResolvedFilter::Term(_) => from_ref(&resolved),
    };
    conjuncts
        .iter()
        .filter_map(|conjunct| match conjunct {
            ResolvedFilter::Term(term) => candidates_of(term, rows).map(|(select, read)| {
                (
                    KeyedCandidates {
                        term: term.term(),
                        select,
                        read,
                    },
                    term.carrier(),
                )
            }),
            ResolvedFilter::All(_) | ResolvedFilter::Any(_) => None,
        })
        .collect()
}

/// The candidate statement of one term and what reading it costs, `None`
/// when no index answers it.
///
/// A hop is read inward, whatever its inner term compares. A complement
/// (`!=`, `!~=`) never narrows: an Entity holds when none of its values pass
/// the test, which no index of passing values can enumerate.
fn candidates_of(term: &ResolvedTerm<'_>, rows: &EntityRowRefs) -> Option<(SelectStatement, CandidateRead)> {
    let rechecked = |select| (select, CandidateRead::Rechecked);
    let ranged = |select| (select, CandidateRead::Ranged);
    let probed = |select| (select, CandidateRead::Probed);
    if !term.hops().is_empty() {
        return hop_candidates(term, rows).map(|select| (select, CandidateRead::Linked));
    }
    let value = match term.term().test() {
        Test::Exists => return Some(probed(existence_candidates(own_value_attribute(term)?, rows))),
        Test::Value(value) => value,
    };
    match (value.test(), value.polarity()) {
        (ScalarTest::EqualsAny(operands), Polarity::Asserted) => match operands.as_slice() {
            [operand] => equality_candidates(term, operand, rows).map(rechecked),
            [] | [_, _, ..] => None,
        },
        (ScalarTest::Ordered { .. } | ScalarTest::Within(_), Polarity::Asserted) => {
            ordering_candidates(own_value_attribute(term)?, value.test(), rows).map(ranged)
        }
        (ScalarTest::Matches(pattern), Polarity::Asserted) => pattern_candidates(own_value_attribute(term)?, pattern, rows).map(rechecked),
        (ScalarTest::Unsatisfiable, Polarity::Asserted)
        | (
            ScalarTest::EqualsAny(_) | ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable,
            Polarity::Complement,
        ) => None,
    }
}

/// The attribute whose own value `term` tests, `None` when it tests a value
/// past a hop, below a sub-attribute or inside the value: those are elements
/// of an instance, which the indexes over a row's own value do not read.
fn own_value_attribute(term: &ResolvedTerm<'_>) -> Option<AttributeNameKey> {
    let reading: &TermReading = term.term();
    (reading.hops().is_empty() && reading.sub_attributes().is_empty() && matches!(reading.terminal(), Terminal::OwnValue)).then(|| term.attribute())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_carriers::AttributeCarriers,
        entity_row_refs::EntityRowRefs,
        one_time_gate::CandidateBound,
        q_predicate::{
            keyed_term::{CandidateRead, contended_carriers, keyed_candidates},
            lowered_filter::reading_of,
        },
        query_names::QueryNames,
    };
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};

    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::SelectStatement;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("name", 2), ("source", 7), ("count", 9), ("observedOn", 11), ("hasTrip", 13)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates(q: &str) -> Option<SelectStatement> {
        keyed_candidates(&reading_of(q), &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole).map(|keyed| keyed.select)
    }

    fn read(q: &str) -> Option<CandidateRead> {
        keyed_candidates(&reading_of(q), &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole).map(|keyed| keyed.read)
    }

    /// A trigram or key match is rechecked against the value; a range of the
    /// typed value index is a match as it is read; a carrier of the name is
    /// probed for a standing instance.
    #[test]
    fn each_term_states_what_reading_one_candidate_costs() {
        for (q, expected) in [
            (
                "\"https://example.com/name\".\"https://example.com/source\"==\"sensor-43\"",
                CandidateRead::Rechecked,
            ),
            ("\"https://example.com/name\"~=\"text-71.*\"", CandidateRead::Rechecked),
            ("\"https://example.com/observedOn\">2024-01-15T12:00:00Z", CandidateRead::Ranged),
            ("\"https://example.com/observedOn\"==2024-01-01..2024-02-01", CandidateRead::Ranged),
            ("\"https://example.com/count\"", CandidateRead::Probed),
            ("\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"", CandidateRead::Linked),
        ] {
            assert_eq!(read(q), Some(expected), "{q}");
        }
    }

    #[test]
    fn the_candidate_statement_reads_every_candidate() {
        for (index, q) in [
            "\"https://example.com/name\".\"https://example.com/source\"==\"sensor-43\"",
            "\"https://example.com/name\"~=\"text-71.*\"",
            "\"https://example.com/observedOn\">2024-01-15T12:00:00Z",
            "\"https://example.com/observedOn\"==2024-01-01..2024-02-01",
            "\"https://example.com/count\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"",
        ]
        .into_iter()
        .enumerate()
        {
            let statement = candidates(q).unwrap_or_else(|| panic!("keyed: {q}"));
            assert_statement_snapshot!(format!("keyed-term-the-candidate-statement-reads-every-candidate-{}", index + 1), statement);
        }
    }

    #[test]
    fn a_term_inside_a_disjunction_is_not_keyed() {
        for q in [
            "\"https://example.com/name\"~=\"a.*\"|\"https://example.com/count\"==1",
            "\"https://example.com/name\".\"https://example.com/source\"==\"s\"|\"https://example.com/count\"==1",
            "\"https://example.com/observedOn\">2024-01-15|\"https://example.com/count\"==1",
            "\"https://example.com/count\"|\"https://example.com/name\"==\"x\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"|\"https://example.com/count\"==1",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn the_first_keyed_conjunct_is_the_one_the_index_answers() {
        let statement = candidates(
            "\"https://example.com/count\">1;\"https://example.com/name\"~=\"text-71.*\";\"https://example.com/name\".\"https://example.com/source\"==\"s\"",
        )
        .expect("keyed");
        assert_statement_snapshot!("keyed-term-the-first-keyed-conjunct-is-the-one-the-index-answers", statement);
    }

    #[test]
    fn an_existence_conjunct_is_keyed_after_an_unkeyed_one() {
        let statement = candidates("\"https://example.com/count\">1;\"https://example.com/name\"").expect("keyed");
        assert_statement_snapshot!("keyed-term-an-existence-conjunct-is-keyed-after-an-unkeyed-one", statement);
    }

    #[test]
    fn a_hop_conjunct_is_keyed_after_an_unkeyed_one() {
        let statement = candidates("\"https://example.com/count\">1;\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"").expect("keyed");
        assert_statement_snapshot!("keyed-term-a-hop-conjunct-is-keyed-after-an-unkeyed-one", statement);
    }

    #[test]
    fn a_date_ordering_conjunct_is_keyed_after_an_unkeyed_one() {
        let statement = candidates("\"https://example.com/count\">1;\"https://example.com/observedOn\"<2024-01-15").expect("keyed");
        assert_statement_snapshot!("keyed-term-a-date-ordering-conjunct-is-keyed-after-an-unkeyed-one", statement);
    }

    #[test]
    fn a_term_no_index_answers_is_not_keyed() {
        for q in [
            "\"https://example.com/count\">1",
            "\"https://example.com/name\"!~=\"text-71.*\"",
            "\"https://example.com/name\">\"m\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/name\"}!=\"x\"",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    /// The names of the census tests: `count` (key 9) and `name` (key 2).
    const COUNT_AND_NAME: &str = "\"https://example.com/count\";\"https://example.com/name\"~=\"text-.*\"";

    /// The attribute key of the conjunct taken for `q` in normal form, under
    /// the census `counts` and a bound of 1 000 for every read.
    fn keyed_attribute(q: &str, counts: &[(i32, u64)]) -> Option<String> {
        let mut names = names();
        let read: Vec<AttributeNameKey> = counts.iter().map(|(key, _)| AttributeNameKey::from(*key)).collect();
        names.carriers = AttributeCarriers::new(&read, counts.iter().map(|(key, count)| (AttributeNameKey::from(*key), *count)));
        let reading = FilterReading::of(&parse(q).expect("the filter parses"), LanguageMatching::TagEquality);
        keyed_candidates(&reading, &EntityRowRefs::unaliased(None), &names, |_| CandidateBound::AtMost(1000))
            .map(|keyed| keyed.term.attribute().as_str().to_owned())
    }

    /// The census bounds a conjunct's candidates by the instances of the
    /// attribute it is read through, so a count within the bound is answered
    /// by the index whole, ahead of the canonical first conjunct, whose count
    /// is past it.
    #[test]
    fn a_count_within_its_bound_is_keyed_ahead_of_the_canonical_first_conjunct() {
        assert_eq!(
            keyed_attribute(COUNT_AND_NAME, &[(9, 100_000), (2, 50)]).as_deref(),
            Some("https://example.com/name")
        );
    }

    #[test]
    fn of_two_counts_within_their_bounds_the_smaller_is_keyed() {
        assert_eq!(
            keyed_attribute(COUNT_AND_NAME, &[(9, 10), (2, 500)]).as_deref(),
            Some("https://example.com/count")
        );
        assert_eq!(
            keyed_attribute(COUNT_AND_NAME, &[(9, 500), (2, 10)]).as_deref(),
            Some("https://example.com/name")
        );
    }

    /// A count past the bound says nothing of how few Entities a value test
    /// keeps, so the canonical first conjunct stays keyed.
    #[test]
    fn with_no_count_within_its_bound_the_canonical_first_conjunct_is_keyed() {
        assert_eq!(
            keyed_attribute(COUNT_AND_NAME, &[(9, 5000), (2, 5000)]).as_deref(),
            Some("https://example.com/count")
        );
        assert_eq!(keyed_attribute(COUNT_AND_NAME, &[]).as_deref(), Some("https://example.com/count"));
    }

    #[test]
    fn the_census_is_read_only_when_two_conjuncts_contend() {
        let reading = |q: &str| FilterReading::of(&parse(q).expect("the filter parses"), LanguageMatching::TagEquality);
        assert_eq!(
            contended_carriers(&reading(COUNT_AND_NAME), &names()),
            [AttributeNameKey::from(2), AttributeNameKey::from(9)]
        );
        assert_eq!(
            contended_carriers(&reading("\"https://example.com/count\""), &names()),
            Vec::<AttributeNameKey>::new()
        );
        assert_eq!(
            contended_carriers(&reading("\"https://example.com/count\"|\"https://example.com/name\"~=\"x\""), &names()),
            Vec::<AttributeNameKey>::new(),
            "a disjunction offers no conjunct"
        );
    }
}
