//! Whether a Context Source Registration covers the entities a distributed
//! operation concerns (clauses 4.3.6 and 5.12).
//!
//! The forwarding decision asks the inverse of discovery: not "which
//! registrations did a client ask for" but "which registrations declared they
//! may hold the entities a request concerns". The entity axis is the clause
//! 5.12 specification discovery renders too, and the attribute names an item
//! declares hold for each of its entries.
//!
//! One `registration_information` row is one `EntityInfo` with its item's
//! names, so the axes asked about are decided together on one row. The result
//! is a superset the caller decides with the typed registration, which matches
//! per item and per `EntityInfo` as well.

use crate::csr::{entity_specification::row_matches, information_row::some_information_row, preselect::attribute_name_candidate};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::csr::expanded_entity_specification::ExpandedEntitySpecification;
use sea_query::{Condition, SimpleExpr};

/// What a distributed operation asks of a registration's `information`.
#[derive(Debug, Clone, Copy)]
pub struct Coverage<'request> {
    /// The entities the operation concerns.
    pub entities: ExpandedEntitySpecification<'request>,
    /// The expanded attribute names; empty when none are asked about.
    pub attributes: &'request [AttributeName],
}

impl<'request> Coverage<'request> {
    /// Whether some `registration_information` row of the registration covers
    /// every axis asked about, or `None` when nothing is asked.
    #[must_use]
    pub fn predicate(&self) -> Option<SimpleExpr> {
        let axes = [
            row_matches(&self.entities),
            (!self.attributes.is_empty()).then(|| Condition::all().add(attribute_name_candidate(self.attributes))),
        ];
        let row = axes.into_iter().flatten().fold(Condition::all(), Condition::add);
        (!row.is_empty()).then(|| some_information_row(row))
    }

    /// Whether the registration covers at least one of `entities` (each a
    /// clause 5.12 specification of a selector in the union a query asks for)
    /// together with `attributes`, or `None` when nothing is asked.
    ///
    /// A specification that constrains nothing admits every registration, so one
    /// among the others leaves the whole unconstrained; an empty slice asks
    /// nothing either.
    #[must_use]
    pub fn any_of(entities: &[ExpandedEntitySpecification<'request>], attributes: &'request [AttributeName]) -> Option<SimpleExpr> {
        let alternatives = entities
            .iter()
            .map(|entities| {
                Coverage {
                    entities: *entities,
                    attributes,
                }
                .predicate()
            })
            .collect::<Option<Vec<_>>>()?;
        (!alternatives.is_empty()).then(|| alternatives.into_iter().fold(Condition::any(), Condition::add).into())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExpr};
    use andromeda_postgresql_seaquery::literal::int_literal;
    use iri_rs::IriBuf;
    use sea_query::{PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(coverage: &Coverage<'_>) -> String {
        Query::select()
            .expr(int_literal(1))
            .from(sea_query::Alias::new("context_source_registrations"))
            .and_where(coverage.predicate().expect("something is asked"))
            .to_string(PostgresQueryBuilder)
    }

    fn entities(coverage: ExpandedEntitySpecification<'_>) -> String {
        render(&Coverage {
            entities: coverage,
            attributes: &[],
        })
    }

    #[test]
    fn any_of_joins_one_row_existence_check_per_specification() {
        let alpha = TypeExpr::Type(iri("https://example.com/Alpha"));
        let beta = TypeExpr::Type(iri("https://example.com/Beta"));
        let specifications = [
            ExpandedEntitySpecification {
                entity_type: Some(&alpha),
                ..ExpandedEntitySpecification::default()
            },
            ExpandedEntitySpecification {
                entity_type: Some(&beta),
                ..ExpandedEntitySpecification::default()
            },
        ];
        let sql = Query::select()
            .expr(int_literal(1))
            .from(sea_query::Alias::new("context_source_registrations"))
            .and_where(Coverage::any_of(&specifications, &[]).expect("something is asked"))
            .to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql.matches("EXISTS(").count(), 2, "{sql}");
        assert!(sql.contains("https://example.com/Alpha") && sql.contains("https://example.com/Beta"), "{sql}");
        assert!(sql.contains(") OR "), "{sql}");
    }

    #[test]
    fn any_of_is_unconstrained_when_one_specification_asks_nothing_or_none_is_given() {
        let alpha = TypeExpr::Type(iri("https://example.com/Alpha"));
        let specifications = [
            ExpandedEntitySpecification {
                entity_type: Some(&alpha),
                ..ExpandedEntitySpecification::default()
            },
            ExpandedEntitySpecification::default(),
        ];
        assert!(Coverage::any_of(&specifications, &[]).is_none());
        assert!(Coverage::any_of(&[], &[]).is_none());
    }

    #[test]
    fn type_coverage_admits_a_row_without_entities_and_a_row_declaring_the_type() {
        let vehicle = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let sql = entities(ExpandedEntitySpecification {
            entity_type: Some(&vehicle),
            ..ExpandedEntitySpecification::default()
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Vehicle','*']"),
            "{sql}"
        );
    }

    /// Clause 5.12, first to third bullets: a row constraining the identifier
    /// not at all, a row naming one of the identifiers, and a row whose own
    /// pattern one of them matches, with clause 5.2.8 consulting that pattern
    /// only on a row without an `id`.
    #[test]
    fn id_coverage_admits_an_exact_id_an_unconstrained_row_and_a_matching_pattern_only() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let sql = entities(ExpandedEntitySpecification {
            ids: &ids,
            ..ExpandedEntitySpecification::default()
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1',''])"),
            "{sql}"
        );
        assert!(sql.contains("\"ri\".\"entity_id\" = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1'])"), "{sql}");
        assert!(
            sql.contains("\"ri\".\"entity_id\" IS NULL AND (\"ri\".\"entity_id_pattern_form\" IS NULL OR EXISTS("),
            "{sql}"
        );
        assert!(
            !sql.contains("\"entity_id_pattern\" ~"),
            "the registered pattern is evaluated as its stored restatement: {sql}"
        );
    }

    /// Every identifier a request names decides the row, so a registration
    /// covering only the second of them is a candidate.
    #[test]
    fn id_coverage_names_every_identifier_the_request_names() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let sql = entities(ExpandedEntitySpecification {
            ids: &ids,
            ..ExpandedEntitySpecification::default()
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2',''])"),
            "{sql}"
        );
        assert!(sql.contains("UNNEST(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2'])"), "{sql}");
    }

    /// Clause 5.12, fourth and fifth bullets: the pattern is matched against
    /// the row's `id` through the trigram-indexed column, and a row without an
    /// `id` is a candidate.
    #[test]
    fn pattern_coverage_matches_the_registered_id_and_admits_a_row_without_one() {
        let pattern = Pattern::parse("urn:ngsi-ld:Vehicle:1.*").expect("valid pattern");
        let vehicle = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let sql = entities(ExpandedEntitySpecification {
            entity_type: Some(&vehicle),
            id_pattern: Some(&pattern),
            ..ExpandedEntitySpecification::default()
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') = ('') OR (\"ri\".\"entity_id\" ~ 'urn:ngsi-ld:Vehicle:1.*')"),
            "{sql}"
        );
        assert!(sql.contains("\"entity_types\""), "{sql}");
    }

    /// Clause 5.12: the type, the identifier and the attributes are decided
    /// against the same `EntityInfo` and its item.
    #[test]
    fn every_axis_asked_is_decided_on_one_information_row() {
        let vehicle = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let attributes = ["https://example.com/speed".parse::<AttributeName>().expect("valid attribute name")];
        let sql = render(&Coverage {
            entities: ExpandedEntitySpecification {
                entity_type: Some(&vehicle),
                ids: &ids,
                id_pattern: None,
            },
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
            entities: ExpandedEntitySpecification::default(),
            attributes: &[],
        };
        assert!(coverage.predicate().is_none());
    }
}
