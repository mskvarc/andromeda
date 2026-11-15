//! The index path of a Linked Entity hop (clause 4.9 `rel{attr}`): the
//! Entities whose Relationship names a linked entity the inner term keeps.
//!
//! As a filter, a hop probes each Entity's Relationship instances outward to
//! the entities they name, so its cost follows the selection. The inner term
//! is a test its own index answers, and an index over the object reaches the
//! instances naming each entity it keeps, so read inward the candidates cost
//! the linked matches and the instances naming them. Each candidate is a match
//! as it is read.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{keyed_branches::union_of, projection::linked_condition, resolved_filter::ResolvedTerm, traversal::LinkedHop},
};
use andromeda_ngsi_ld_query::filter::reading::{term_reading::Test, value_test::Polarity};
use sea_query::SelectStatement;

/// `SELECT "entity_key"` over the Entities whose standing Relationship
/// instance names a linked entity `term` keeps, `None` unless `term` crosses
/// one hop and its test is one an index can enumerate.
///
/// A complement (`!=`, `!~=`) holds for an entity none of whose values pass
/// the test, which no index of values enumerates, and a further hop is itself
/// a probe outward from every entity, so either would read every entity of
/// the tenant before the first candidate.
pub(crate) fn hop_candidates(term: &ResolvedTerm<'_>, rows: &EntityRowRefs) -> Option<SelectStatement> {
    let [hop] = term.hops() else {
        return None;
    };
    match term.term().test() {
        Test::Exists => {}
        Test::Value(value) => match value.polarity() {
            Polarity::Asserted => {}
            Polarity::Complement => return None,
        },
    }
    let linked = LinkedHop::at_depth(1);
    let inner_rows = linked.inner_rows(rows);
    let inner_condition = linked_condition(term, &inner_rows, 1);
    union_of(linked.linking_branches(hop.relationship(), hop.guard(), rows, inner_condition))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        q_predicate::{keyed_hop::hop_candidates, lowered_filter::term_of, resolved_filter::ResolvedTerm},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, SelectStatement};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("hasTrip", 34), ("headSign", 28), ("hasRoute", 26)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::from_pairs(
                [("Trip", 10), ("Bus", 12)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), EntityTypeNameKey::from(key))),
            ),
            ..QueryNames::default()
        }
    }

    fn candidates_in(q: &str, rows: &EntityRowRefs) -> Option<SelectStatement> {
        let term = term_of(q);
        ResolvedTerm::of(&term, &names()).and_then(|resolved| hop_candidates(&resolved, rows))
    }

    fn candidates(q: &str) -> Option<SelectStatement> {
        candidates_in(q, &EntityRowRefs::unaliased(None))
    }

    const HOP: &str = r#""https://example.com/hasTrip"{"https://example.com/headSign"}=="Vajnory""#;

    #[test]
    fn a_hop_reads_the_instances_naming_the_linked_entities_its_inner_term_keeps() {
        let statement = candidates(HOP).expect("keyed");
        assert_statement_snapshot!("keyed-hop-a-hop-reads-the-instances-naming-the-linked-entities-its-inner-term-keeps", statement);
    }

    /// Clause 4.22: a past-due Relationship instance links nothing, and only
    /// a `Relationship` or a `ListRelationship` holds objects.
    #[test]
    fn each_branch_keeps_the_standing_relationship_instances_and_the_inner_term() {
        let statement = candidates(HOP).expect("keyed");
        assert_statement_snapshot!("keyed-hop-each-branch-keeps-the-standing-relationship-instances-and-the-inner-term", statement);
    }

    #[test]
    fn a_type_guard_reads_the_linked_entitys_search_row_in_each_branch() {
        let statement =
            candidates(r#""https://example.com/hasTrip"{"https://example.com/Trip","https://example.com/Bus":"https://example.com/headSign"}=="Vajnory""#)
                .expect("keyed");
        assert_statement_snapshot!("keyed-hop-a-type-guard-reads-the-linked-entitys-search-row-in-each-branch", statement);
    }

    #[test]
    fn the_linked_entities_are_read_in_the_branch_schema_of_the_statement() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snapshot_1").into_iden()));
        let statement = candidates_in(HOP, &rows).expect("keyed");
        assert_statement_snapshot!("keyed-hop-the-linked-entities-are-read-in-the-branch-schema-of-the-statement", statement);
    }

    #[test]
    fn a_positive_inner_term_of_any_operator_is_keyed() {
        for q in [
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}"#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}>3"#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}~="Vaj.*""#,
        ] {
            assert!(candidates(q).is_some(), "{q}");
        }
    }

    #[test]
    fn a_negated_inner_term_or_a_further_hop_is_not_keyed() {
        for q in [
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}!="Vajnory""#,
            r#""https://example.com/hasTrip"{"https://example.com/headSign"}!~="Vaj.*""#,
            r#""https://example.com/hasTrip"{"https://example.com/hasRoute"{"https://example.com/headSign"}}=="Vajnory""#,
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }

    #[test]
    fn a_hop_through_an_unrecorded_relationship_or_a_local_term_is_not_keyed() {
        for q in [
            r#""https://example.com/never-written"{"https://example.com/headSign"}=="Vajnory""#,
            r#""https://example.com/headSign"=="Vajnory""#,
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
    }
}
