//! The index path of an equality with one operand on an attribute's own value:
//! the rows of the attribute that hold the operand as their target value.
//!
//! The term compiles to one disjunction over the typed fields the operand
//! compares with and the structured value ([`EqualityArms`]), which no one
//! index drives. The candidates read each arm on its own instead, as one
//! branch of a `UNION ALL`: a typed field through the partial B-tree over it,
//! keyed by the attribute and the value, and the structured value through the
//! containment keys of `idx_ca_value_object`, which the arm rechecks.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        equality_term::{EqualityArms, equality_arms},
        instance_row::InstanceRow,
        keyed_branches::{branch, union_of},
    },
};
use andromeda_ngsi_ld_query::filter::reading::operand::Operand;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, SelectStatement};

/// `SELECT "entity_key"` over the rows of `attribute` whose own value equals
/// `operand`, one branch per arm of the equality.
pub(crate) fn value_equality_candidates(attribute: AttributeNameKey, operand: &Operand, rows: &EntityRowRefs) -> Option<SelectStatement> {
    let EqualityArms { fields, structured } = equality_arms(operand, InstanceRow::Current);
    let fields = fields.into_iter().map(|field| branch(rows, attribute, None, Condition::all().add(field)));
    union_of(fields.chain([branch(rows, attribute, None, structured)]))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        one_time_gate::CandidateBound,
        q_predicate::{keyed_term::keyed_candidates, lowered_filter::reading_of},
        query_names::QueryNames,
    };
    use andromeda_postgresql_schema::string_value_column::INDEXED_STRING_BYTE_LIMIT;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use sea_query::SelectStatement;

    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("count", 9), ("flag", 10), ("name", 2), ("link", 13), ("observedOn", 11)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn candidates(q: &str) -> SelectStatement {
        keyed_candidates(&reading_of(q), &EntityRowRefs::unaliased(None), &names(), |_| CandidateBound::Whole)
            .map_or_else(|| panic!("keyed: {q}"), |keyed| keyed.select)
    }

    /// Every branch reads the standing rows of the attribute alone, so an
    /// instance past its clause 4.22 `expiresAt` is no candidate.
    #[test]
    fn a_number_equality_reads_the_number_index_and_the_containment_index() {
        assert_statement_snapshot!(
            "keyed-value-equality-a-number-equality-reads-the-number-index-and-the-containment-index",
            candidates("\"https://example.com/count\"==1")
        );
    }

    #[test]
    fn a_boolean_equality_reads_the_boolean_index_with_its_predicate() {
        assert_statement_snapshot!(
            "keyed-value-equality-a-boolean-equality-reads-the-boolean-index-with-its-predicate",
            candidates("\"https://example.com/flag\"==false")
        );
    }

    #[test]
    fn a_text_equality_reads_the_string_and_uri_indexes() {
        assert_statement_snapshot!(
            "keyed-value-equality-a-text-equality-reads-the-string-and-uri-indexes",
            candidates("\"https://example.com/name\"==\"fail\"")
        );
    }

    #[test]
    fn a_long_text_equality_reads_the_long_text_column() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        assert_statement_snapshot!(
            "keyed-value-equality-a-long-text-equality-reads-the-long-text-column",
            candidates(&format!("\"https://example.com/name\"==\"{long}\""))
        );
    }

    #[test]
    fn a_uri_equality_reads_the_uri_index() {
        assert_statement_snapshot!(
            "keyed-value-equality-a-uri-equality-reads-the-uri-index",
            candidates("\"https://example.com/link\"==urn:ngsi-ld:Device:hub")
        );
    }

    #[test]
    fn a_date_time_equality_reads_the_date_time_index() {
        assert_statement_snapshot!(
            "keyed-value-equality-a-date-time-equality-reads-the-date-time-index",
            candidates("\"https://example.com/observedOn\"==2024-01-15T12:00:00Z")
        );
    }
}
