//! Whether a Context Source Registration covers a concrete entity (clause 5.10.2).
//!
//! The forwarding decision asks the inverse of discovery: not "which
//! registrations did a client ask for" but "which registrations declared they
//! hold a given entity". An `EntityInfo` constrains the identifier by `id` or
//! `idPattern` and the kind by `type`, an entry that constrains an axis not at
//! all covers every value of it, and an item without `entities` covers every
//! entity. The attribute names an item declares hold for each of its entries.
//!
//! One `registration_information` row is one `EntityInfo` with its item's
//! names, so the axes asked about are decided together on one row. The result
//! is a superset the caller decides with the typed registration, which matches
//! per item and per `EntityInfo` as well.

use crate::csr::{
    information_row::{ri_col, some_information_row},
    preselect::{attribute_name_candidate, entity_id_candidate, entity_type_candidate},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::csr::RegistrationInformation;
use andromeda_postgresql_seaquery::operator::regex_match;
use iri_rs::IriBuf;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};
use std::slice;

/// What an entity a forwarded request concerns asks of a registration's
/// `information`.
#[derive(Debug, Clone, Copy)]
pub struct Coverage<'request> {
    /// The expanded entity type, if the request resolves one.
    pub entity_type: Option<&'request IriBuf>,
    /// The entity identifier, if the request names one.
    pub entity_id: Option<&'request Identifier>,
    /// The expanded attribute names; empty when none are asked about.
    pub attributes: &'request [IriBuf],
}

impl Coverage<'_> {
    /// Whether some `registration_information` row of the registration covers
    /// every axis asked about, or `None` when nothing is asked.
    #[must_use]
    pub fn predicate(&self) -> Option<SimpleExpr> {
        let axes = [
            self.entity_type.map(|entity_type| entity_type_candidate(slice::from_ref(entity_type))),
            self.entity_id.map(identifier_covered),
            (!self.attributes.is_empty()).then(|| attribute_name_candidate(self.attributes)),
        ];
        let row = axes.into_iter().flatten().fold(Condition::all(), Condition::add);
        (!row.is_empty()).then(|| some_information_row(row))
    }
}

/// Whether the row covers `entity_id`: it constrains the identifier by neither
/// `id` nor `idPattern`, its `id` equals the entity's, or its `idPattern`
/// matches it.
///
/// A pattern-only row covers only the identifiers its pattern admits, so a row
/// without an `id` is not on its own a cover. The pattern is evaluated as its
/// stored restatement, so `(?i)` folds as the in-memory engine folds it, and
/// only on rows the indexed identifier test admits.
fn identifier_covered(entity_id: &Identifier) -> SimpleExpr {
    let covered = Condition::any()
        .add(
            Condition::all()
                .add(ri_col(RegistrationInformation::EntityId).is_null())
                .add(ri_col(RegistrationInformation::EntityIdPatternForm).is_null()),
        )
        .add(ri_col(RegistrationInformation::EntityId).eq(entity_id.as_str()))
        .add(regex_match(
            SeaExpr::val(entity_id.as_str()),
            ri_col(RegistrationInformation::EntityIdPatternForm),
        ));
    entity_id_candidate(slice::from_ref(entity_id)).and(SimpleExpr::from(covered))
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

    fn render(coverage: &Coverage<'_>) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(sea_query::Alias::new("context_source_registrations"))
            .and_where(coverage.predicate().expect("something is asked"))
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn type_coverage_admits_a_row_without_entities_and_a_row_declaring_the_type() {
        let vehicle = iri("https://example.com/Vehicle");
        let sql = render(&Coverage {
            entity_type: Some(&vehicle),
            entity_id: None,
            attributes: &[],
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Vehicle','*']"),
            "{sql}"
        );
    }

    #[test]
    fn id_coverage_admits_an_exact_id_an_unconstrained_row_and_a_matching_pattern_only() {
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let sql = render(&Coverage {
            entity_type: None,
            entity_id: Some(&id),
            attributes: &[],
        });
        assert!(
            sql.contains("\"ri\".\"entity_id\" IS NULL AND \"ri\".\"entity_id_pattern_form\" IS NULL"),
            "{sql}"
        );
        assert!(sql.contains("\"ri\".\"entity_id\" = 'urn:ngsi-ld:Vehicle:1'"), "{sql}");
        assert!(sql.contains("COALESCE(\"ri\".\"entity_id\", '') IN ('urn:ngsi-ld:Vehicle:1', '')"), "{sql}");
    }

    /// The registered pattern is evaluated as its stored restatement, never as
    /// the text the client wrote.
    #[test]
    fn id_coverage_evaluates_the_stored_restatement_of_the_registered_pattern() {
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let sql = render(&Coverage {
            entity_type: None,
            entity_id: Some(&id),
            attributes: &[],
        });
        assert!(sql.contains("'urn:ngsi-ld:Vehicle:1' ~ \"ri\".\"entity_id_pattern_form\""), "{sql}");
        assert!(!sql.contains("\"entity_id_pattern\" ~"), "{sql}");
    }

    /// Clause 5.12: the type, the identifier and the attributes are decided
    /// against the same `EntityInfo` and its item.
    #[test]
    fn every_axis_asked_is_decided_on_one_information_row() {
        let vehicle = iri("https://example.com/Vehicle");
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let attributes = [iri("https://example.com/speed")];
        let sql = render(&Coverage {
            entity_type: Some(&vehicle),
            entity_id: Some(&id),
            attributes: &attributes,
        });
        pretty_assertions::assert_eq!(sql.matches("FROM \"registration_information\"").count(), 1, "{sql}");
        assert!(sql.contains("\"entity_types\""), "{sql}");
        assert!(sql.contains("\"entity_id_pattern_form\""), "{sql}");
        assert!(sql.contains("\"property_name_keys\""), "{sql}");
        assert!(!sql.contains("jsonb"), "{sql}");
    }

    #[test]
    fn nothing_asked_is_no_predicate() {
        let coverage = Coverage {
            entity_type: None,
            entity_id: None,
            attributes: &[],
        };
        assert!(coverage.predicate().is_none());
    }
}
