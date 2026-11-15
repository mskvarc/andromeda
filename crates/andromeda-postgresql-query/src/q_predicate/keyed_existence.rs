//! The index path of a bare existence term (clause 4.9 `q=attr`): the
//! Entities that carry a standing instance of the attribute.
//!
//! An Entity's search row names every attribute member it carries, as
//! dictionary keys the GIN over them yields by key. A name stays listed after
//! its last instance's clause 4.22 `expiresAt` passes, until the sweep rewrites
//! the Entity, so each carrier is probed for a standing instance on the leading
//! columns of the grain constraint. The candidates are then exactly the
//! Entities the term keeps.

use crate::{entity_row_refs::EntityRowRefs, q_predicate::projection_probe::exists_on_projection};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::operator::array_contains_key;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, Query, SelectStatement};

/// `SELECT "entity_key"` over the search rows naming `attribute` whose Entity
/// holds a standing instance of it, the candidates of its existence; `None`
/// when the attribute was never recorded.
///
/// Only the existence of an attribute is offered: an element inside an
/// instance, a trailing member or a sub-attribute, is not recorded in the
/// search row, and a hop asks about the Entity the Relationship names.
pub(crate) fn existence_candidates(attribute: AttributeNameKey, rows: &EntityRowRefs) -> SelectStatement {
    let carriers = EntityRowRefs::unaliased(rows.schema().cloned())
        .keyed_by(EntitySearch::Table)
        .standing_at(rows.standing());
    let mut candidates = Query::select();
    candidates
        .expr(carriers.entity_key_column())
        .from(carriers.table(EntitySearch::Table))
        .and_where(array_contains_key(carriers.search_column(EntitySearch::AttributeNameKeys), attribute.get()))
        .cond_where(exists_on_projection(attribute, Condition::all(), &carriers));
    candidates.take()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        one_time_gate::CandidateBound,
        q_predicate::{
            keyed_term::{CandidateRead, keyed_candidates},
            lowered_filter::reading_of,
        },
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, SelectStatement};

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("speed", 7), ("source", 9), ("isParked", 11)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates_in(q: &str, rows: &EntityRowRefs) -> Option<SelectStatement> {
        keyed_candidates(&reading_of(q), rows, &names(), |_| CandidateBound::Whole).map(|keyed| keyed.select)
    }

    fn candidates(q: &str) -> Option<SelectStatement> {
        candidates_in(q, &EntityRowRefs::unaliased(None))
    }

    /// Clause 4.22: a listed name whose every instance has expired is not a
    /// carrier, so the probe keeps only the standing instances.
    #[test]
    fn a_bare_existence_reads_the_carriers_each_probed_for_a_standing_instance() {
        let statement = candidates("\"https://example.com/speed\"").expect("keyed");
        assert_statement_snapshot!(
            "keyed-existence-a-bare-existence-reads-the-carriers-each-probed-for-a-standing-instance",
            statement
        );
    }

    #[test]
    fn the_carriers_are_read_in_the_branch_schema_of_the_statement() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snapshot_1").into_iden()));
        let statement = candidates_in("\"https://example.com/speed\"", &rows).expect("keyed");
        assert_statement_snapshot!("keyed-existence-the-carriers-are-read-in-the-branch-schema-of-the-statement", statement);
    }

    #[test]
    fn an_existence_inside_an_instance_is_not_keyed_and_one_through_a_hop_is_read_inward() {
        for q in [
            "\"https://example.com/speed\".\"https://example.com/source\"",
            "\"https://example.com/speed\"[en]",
        ] {
            assert_eq!(candidates(q), None, "{q}");
        }
        let hop = reading_of("\"https://example.com/isParked\"{\"https://example.com/speed\"}");
        let keyed = keyed_candidates(&hop, &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole);
        assert_eq!(keyed.map(|keyed| keyed.read), Some(CandidateRead::Linked));
    }

    #[test]
    fn an_existence_of_an_unrecorded_attribute_is_not_keyed() {
        assert_eq!(candidates("\"https://example.com/never-written\""), None);
    }

    #[test]
    fn a_comparison_is_not_an_existence() {
        assert_eq!(candidates("\"https://example.com/speed\">1"), None);
    }
}
