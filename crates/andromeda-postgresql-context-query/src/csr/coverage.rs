//! Whether a Context Source Registration covers a concrete entity (clause 5.10.2).
//!
//! The forwarding decision asks the inverse of discovery: not "which
//! registrations did a client ask for" but "which registrations declared they
//! hold a given entity". A registration's `information[].entities[]` may
//! constrain the identifier by `id` or `idPattern` and the kind by `type`, and
//! an entry that constrains an axis not at all covers every value of it — so a
//! registration with no `entities`, or an empty list, covers everything.
//!
//! The `type` predicate is shared with the clause-5.9 discovery query, which
//! asks the same "does any entry declare this type" question, so the two cannot
//! drift on what a type registration means.
//!
//! Each predicate is a conjunction of two tests that decide the same question at
//! different costs. The indexed preselect over the routing projection runs
//! first over one row per information item per entity selector and discards
//! registrations no unnesting could have matched; the
//! `information` unnesting that follows stays the authority on what the
//! registration actually covers. Because the preselect is a superset of the
//! unnesting (see [`crate::csr::projection`]), conjoining it leaves the result
//! set unchanged and only spares the unnesting the rows it would have rejected.

use crate::csr::{
    entity_specification::entity_type_admits,
    projection::{entity_id_candidate, entity_type_candidate},
    unnesting::{ent, entities_of_item, information_covers, lacks_key, registered_pattern_form},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_seaquery::{jsonb::jsonb_exists, literal::text_literal, operator::regex_match, pg_operator::PgBinaryOperator};
use iri_rs::IriBuf;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};
use std::slice;

/// Whether a registration's `information[].entities[]` covers `entity_type`.
///
/// A registration whose `information` entry declares no `entities`, or an empty
/// list, places no type constraint and covers every type; otherwise at least
/// one entry must declare the type, alone or among several (Table 5.2.8-1).
/// The type is the expanded entity-type IRI the query was compiled against.
///
/// The projection preselect narrows the candidates the unnesting has to read;
/// the unnesting decides coverage.
#[must_use]
pub fn entity_type_coverage(entity_type: &IriBuf) -> SimpleExpr {
    let mut entities_match = entities_of_item();
    entities_match.and_where(entity_type_admits(entity_type));
    entity_type_candidate(entity_type).and(information_covers(entities_match))
}

/// Whether a registration's `information[].entities[]` covers `entity_id`.
///
/// Clause 5.10.2: an `EntityInfo` entry covers a concrete id when it constrains
/// the identifier axis by neither `id` nor `idPattern` (a type-only entry covers
/// any id), when its `id` equals the entity's, or when its `idPattern` regular
/// expression matches it. An `idPattern`-only entry therefore covers only the
/// ids its pattern admits, not every id — the trap a bare `NOT jsonb_exists(ent,
/// 'id')` disjunct falls into, since an entry that carries an `idPattern` but no
/// `id` would otherwise match every entity. The pattern is evaluated as its
/// stored restatement ([`registered_pattern_form`]), so `(?i)` folds as the
/// in-memory engine folds it.
///
/// The projection preselect that runs first admits every pattern row without
/// looking at the pattern, so the regular expression is evaluated exactly once,
/// here, where the coverage decision is made.
#[must_use]
pub fn entity_id_coverage(entity_id: &Identifier) -> SimpleExpr {
    let mut entities_match = entities_of_item();
    entities_match.cond_where(
        Condition::any()
            .add(Condition::all().add(lacks_key(ent(), "id")).add(lacks_key(ent(), "idPattern")))
            .add(SeaExpr::expr(ent().json_get_text(text_literal("id"))).eq(entity_id.as_str()))
            .add(
                Condition::all()
                    .add(jsonb_exists(ent(), text_literal("idPattern")))
                    .add(regex_match(SeaExpr::val(entity_id.as_str()), registered_pattern_form())),
            ),
    );
    entity_id_candidate(slice::from_ref(entity_id)).and(information_covers(entities_match))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(predicate: SimpleExpr) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(sea_query::Alias::new("context_source_registrations"))
            .and_where(predicate)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn type_coverage_matches_a_type_only_registration_and_a_declared_type() {
        // A registration with no `entities` (the first two disjuncts) covers any
        // type, and one that declares entities covers a type it names.
        let sql = render(entity_type_coverage(&iri("https://example.com/Vehicle")));
        assert!(sql.contains("jsonb_exists("), "the no-entities disjunct tests key presence: {sql}");
        assert!(sql.contains("'entities'"), "{sql}");
        assert!(
            sql.contains("jsonb_exists(\"ent\".\"value\" -> ('type'), 'https://example.com/Vehicle')"),
            "{sql}"
        );
    }

    #[test]
    fn id_coverage_admits_exact_id_type_only_and_idpattern_but_not_a_bare_pattern_wildcard() {
        // Clause 5.10.2: a type-only entry covers any id, an `id` covers itself,
        // an `idPattern` covers only the ids it matches. The regex disjunct is
        // what stops an idPattern-only entry from covering every id.
        let sql = render(entity_id_coverage(&uri("urn:ngsi-ld:Vehicle:1")));
        assert!(sql.contains("'id'"), "{sql}");
        assert!(sql.contains("'idPattern'"), "{sql}");
        assert!(sql.contains("'urn:ngsi-ld:Vehicle:1'"), "the exact-id disjunct binds the identifier: {sql}");
        assert!(sql.contains(" ~ "), "the idPattern regex disjunct must be present: {sql}");
    }

    /// The registered pattern is evaluated as its stored restatement, looked up
    /// by the entry's own `idPattern` text, never as the text itself.
    #[test]
    fn id_coverage_evaluates_the_stored_restatement_of_the_registered_pattern() {
        let sql = render(entity_id_coverage(&uri("urn:ngsi-ld:Vehicle:1")));
        assert!(
            sql.contains(r#"'urn:ngsi-ld:Vehicle:1' ~ ("context_source_registrations"."entity_id_pattern_forms" ->> ("ent"."value" ->> ('idPattern')))"#),
            "{sql}"
        );
    }

    #[test]
    fn both_coverage_predicates_preselect_against_the_routing_projection() {
        // The projection semi-join narrows the candidates; the `information`
        // unnesting that follows it still decides coverage.
        for sql in [
            render(entity_type_coverage(&iri("https://example.com/Vehicle"))),
            render(entity_id_coverage(&uri("urn:ngsi-ld:Vehicle:1"))),
        ] {
            assert!(sql.contains("\"registration_information\""), "{sql}");
            assert!(sql.contains("jsonb_array_elements(\"information\")"), "{sql}");
        }
    }
}
