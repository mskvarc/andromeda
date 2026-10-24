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
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        keyed_existence::existence_candidates,
        keyed_hop::hop_candidates,
        keyed_ordering::ordering_candidates,
        keyed_pattern::pattern_candidates,
        sub_attribute::keyed_equality::equality_candidates,
    },
};
use andromeda_ngsi_ld_query::filter::{expression::Expr, language_matching::LanguageMatching, predicate::Predicate};
use sea_query::SelectStatement;
use std::slice::from_ref;

/// What reading one candidate costs.
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

/// One term of a `q` expression's top-level conjunction and the candidates
/// the index path reads for it.
#[derive(Debug)]
pub struct KeyedCandidates<'q> {
    /// The term the candidates answer, which the page leaves out of the
    /// filter it applies to them.
    pub term: &'q Predicate,
    /// `SELECT "entity_key" …`, every candidate.
    pub select: SelectStatement,
    /// What reading one candidate costs.
    pub read: CandidateRead,
}

/// The first term of `expr`'s top-level conjunction the index path answers,
/// with its candidates read in the branch `rows` names.
///
/// Only a conjunct can narrow the page to its candidates: an entity a
/// disjunction keeps for another arm need not be one of them. A target
/// language selects `languageMap` keys by `matching`.
#[must_use]
pub fn keyed_candidates<'q>(expr: &'q Expr, rows: &EntityRowRefs, names: &QueryNames, matching: LanguageMatching) -> Option<KeyedCandidates<'q>> {
    let conjuncts = match expr {
        Expr::And(terms) => terms.as_slice(),
        Expr::Or(_) | Expr::Pred(_) => from_ref(expr),
    };
    conjuncts.iter().find_map(|term| match term {
        Expr::Pred(pred) => candidates_of(pred, rows, names, matching).map(|(select, read)| KeyedCandidates { term: pred, select, read }),
        Expr::And(_) | Expr::Or(_) => None,
    })
}

/// The candidate statement of one term and what reading it costs, `None`
/// when no index answers it.
///
/// A hop of any operator is read inward, whatever its inner term compares.
/// `!~=` never narrows: an Entity holds when none of its strings match, which
/// no index of matching strings can enumerate.
fn candidates_of(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames, matching: LanguageMatching) -> Option<(SelectStatement, CandidateRead)> {
    let rechecked = |select| (select, CandidateRead::Rechecked);
    let ranged = |select| (select, CandidateRead::Ranged);
    let probed = |select| (select, CandidateRead::Probed);
    if pred.path().is_linked() {
        return hop_candidates(pred, rows, names, matching).map(|select| (select, CandidateRead::Linked));
    }
    match pred {
        Predicate::Equality { .. } => equality_candidates(pred, rows, names)
            .map(rechecked)
            .or_else(|| ordering_candidates(pred, rows, names).map(ranged)),
        Predicate::Ordering { .. } => ordering_candidates(pred, rows, names).map(ranged),
        Predicate::MatchPattern { .. } => pattern_candidates(pred, rows, names).map(rechecked),
        Predicate::Exists(_) => existence_candidates(pred, rows, names).map(probed),
        Predicate::NotMatchPattern { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_term::{CandidateRead, keyed_candidates},
    };
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

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

    fn candidates(q: &str) -> Option<String> {
        let expr = parse(q).expect("parse q");
        keyed_candidates(&expr, &EntityRowRefs::unaliased(None), &names(), LanguageMatching::TagEquality)
            .map(|keyed| keyed.select.to_string(PostgresQueryBuilder))
    }

    fn read(q: &str) -> Option<CandidateRead> {
        let expr = parse(q).expect("parse q");
        keyed_candidates(&expr, &EntityRowRefs::unaliased(None), &names(), LanguageMatching::TagEquality).map(|keyed| keyed.read)
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
        for q in [
            "\"https://example.com/name\".\"https://example.com/source\"==\"sensor-43\"",
            "\"https://example.com/name\"~=\"text-71.*\"",
            "\"https://example.com/observedOn\">2024-01-15T12:00:00Z",
            "\"https://example.com/observedOn\"==2024-01-01..2024-02-01",
            "\"https://example.com/count\"",
            "\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"",
        ] {
            let sql = candidates(q).unwrap_or_else(|| panic!("keyed: {q}"));
            assert!(!sql.contains("LIMIT"), "{q}: {sql}");
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
        let sql = candidates(
            "\"https://example.com/count\">1;\"https://example.com/name\"~=\"text-71.*\";\"https://example.com/name\".\"https://example.com/source\"==\"s\"",
        )
        .expect("keyed");
        assert!(sql.contains(r#""ca"."value_string" ~ "#), "{sql}");
        assert!(!sql.contains("sub_eq_keys"), "{sql}");
    }

    #[test]
    fn an_existence_conjunct_is_keyed_after_an_unkeyed_one() {
        let sql = candidates("\"https://example.com/count\">1;\"https://example.com/name\"").expect("keyed");
        assert!(sql.contains(r#""entity_search"."attribute_name_keys" @> (ARRAY[2])"#), "{sql}");
    }

    #[test]
    fn a_hop_conjunct_is_keyed_after_an_unkeyed_one() {
        let sql = candidates("\"https://example.com/count\">1;\"https://example.com/hasTrip\"{\"https://example.com/name\"}==\"x\"").expect("keyed");
        assert!(sql.contains(r#""ca_ln1"."value_uri" = "le1"."id""#), "{sql}");
    }

    #[test]
    fn a_date_ordering_conjunct_is_keyed_after_an_unkeyed_one() {
        let sql = candidates("\"https://example.com/count\">1;\"https://example.com/observedOn\"<2024-01-15").expect("keyed");
        assert!(sql.contains(r#""ca"."value_string" < '2024-01-15'"#), "{sql}");
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
}
