//! The one Query Term of a `q` expression an index answers on the page's
//! behalf, and the candidates the index reads for it.
//!
//! Two terms qualify: an equality on a sub-attribute's own value, read
//! through `idx_ca_sub_eq_keys`
//! ([`keyed_equality`](crate::q_predicate::sub_attribute::keyed_equality)),
//! and a `~=` on an attribute's own value, read through the trigram indexes
//! ([`keyed_pattern`](crate::q_predicate::keyed_pattern)). Each candidate is rechecked by the condition the term
//! compiles to, so the candidates are exactly the Entities the term keeps.
//!
//! The candidate statement is unbounded. The planner cannot price either term
//! (it underestimates a broad one by orders of magnitude), and a statement
//! whose matches are many is answered sooner by walking the selection than by
//! fetching every match through the index, so each statement that reads the
//! candidates bounds them by the most it can use and walks past that bound.

use crate::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{keyed_pattern::pattern_candidates, sub_attribute::keyed_equality::equality_candidates},
};
use andromeda_ngsi_ld_query::filter::{expression::Expr, predicate::Predicate};
use sea_query::SelectStatement;
use std::slice::from_ref;

/// One term of a `q` expression's top-level conjunction and the candidates
/// the index path reads for it.
#[derive(Debug)]
pub struct KeyedCandidates<'q> {
    /// The term the candidates answer, which the page leaves out of the
    /// filter it applies to them.
    pub term: &'q Predicate,
    /// `SELECT "entity_key" …`, every candidate.
    pub select: SelectStatement,
}

/// The first term of `expr`'s top-level conjunction the index path answers,
/// with its candidates read in the branch `rows` names.
///
/// Only a conjunct can narrow the page to its candidates: an entity a
/// disjunction keeps for another arm need not be one of them.
#[must_use]
pub fn keyed_candidates<'q>(expr: &'q Expr, rows: &EntityRowRefs, names: &QueryNames) -> Option<KeyedCandidates<'q>> {
    let conjuncts = match expr {
        Expr::And(terms) => terms.as_slice(),
        Expr::Or(_) | Expr::Pred(_) => from_ref(expr),
    };
    conjuncts.iter().find_map(|term| match term {
        Expr::Pred(pred) => candidates_of(pred, rows, names).map(|select| KeyedCandidates { term: pred, select }),
        Expr::And(_) | Expr::Or(_) => None,
    })
}

/// The candidate statement of one term, `None` when no index answers it.
///
/// `!~=` never narrows: an Entity holds when none of its strings match, which
/// no index of matching strings can enumerate.
fn candidates_of(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Option<SelectStatement> {
    match pred {
        Predicate::Equality { .. } => equality_candidates(pred, rows, names),
        Predicate::MatchPattern { .. } => pattern_candidates(pred, rows, names),
        Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::NotMatchPattern { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::keyed_term::keyed_candidates,
    };
    use andromeda_ngsi_ld_query::filter::parser::parse;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("name", 2), ("source", 7), ("count", 9)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
        }
    }

    fn candidates(q: &str) -> Option<String> {
        let expr = parse(q).expect("parse q");
        keyed_candidates(&expr, &EntityRowRefs::unaliased(None), &names()).map(|keyed| keyed.select.to_string(PostgresQueryBuilder))
    }

    #[test]
    fn the_candidate_statement_reads_every_candidate() {
        for q in [
            "\"https://example.com/name\".\"https://example.com/source\"==\"sensor-43\"",
            "\"https://example.com/name\"~=\"text-71.*\"",
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
    fn a_term_no_index_answers_is_not_keyed() {
        for q in [
            "\"https://example.com/count\">1",
            "\"https://example.com/count\"",
            "\"https://example.com/name\"!~=\"text-71.*\"",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }
}
