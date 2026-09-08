//! The clause 5.12 entity specification, rendered against one `EntityInfo`.
//!
//! A discovery query's type selector, entity identifiers and identifier pattern
//! match an `EntityInfo` entry when the type selector matches the entry's
//! type(s) and one of the five clause 5.12 identifier conditions holds. Clause
//! 5.2.8 adds that `id` takes precedence over `idPattern`, so an entry carrying
//! both is matched through its `id` alone.

use crate::csr::unnesting::{ent, entities_of_item, lacks_key};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_postgresql_seaquery::{
    jsonb::jsonb_exists,
    literal::{int_literal, text_literal},
    operator::regex_match,
    pg_array::{any, text_array},
    pg_operator::PgBinaryOperator,
    unnest::unnest_of,
};
use iri_rs::IriBuf;
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The entity axis of a discovery query, as clause 5.12 reads it.
#[derive(Debug, Clone, Copy)]
pub struct EntitySpecification<'query> {
    /// The expanded entity type the selector names, if any.
    pub entity_type: Option<&'query IriBuf>,
    /// The entity identifiers asked for; empty when none were.
    pub ids: &'query [Uri],
    /// The identifier pattern asked for, if any.
    pub id_pattern: Option<&'query Pattern>,
}

impl EntitySpecification<'_> {
    /// Whether the specification constrains the entity axis at all.
    #[must_use]
    pub const fn constrains(&self) -> bool {
        self.entity_type.is_some() || !self.ids.is_empty() || self.id_pattern.is_some()
    }

    /// The `entities` scan admitting the entries this specification matches,
    /// for the caller to wrap in the clause 5.10.2 "no entities covers all"
    /// disjunction.
    #[must_use]
    pub fn matching_entities(&self) -> SelectStatement {
        let mut scan = entities_of_item();
        if let Some(entity_type) = self.entity_type {
            scan.and_where(entity_type_admits(entity_type));
        }
        if let Some(identifier_axis) = self.identifier_axis() {
            scan.cond_where(identifier_axis);
        }
        scan
    }

    /// The clause 5.12 identifier conditions, or `None` when neither
    /// identifiers nor a pattern were asked for.
    fn identifier_axis(&self) -> Option<Condition> {
        if self.ids.is_empty() && self.id_pattern.is_none() {
            return None;
        }
        // Clause 5.12 first bullet: an entry constraining the identifier not at
        // all matches every identifier.
        let mut axis = Condition::any().add(Condition::all().add(lacks_key(ent(), "id")).add(lacks_key(ent(), "idPattern")));
        if !self.ids.is_empty() {
            let ids: Vec<&str> = self.ids.iter().map(Uri::as_str).collect();
            // Second bullet: one of the identifiers equals the entry's `id`.
            axis = axis.add(SeaExpr::expr(registered_id()).eq(any(text_array(&ids))));
            // Third bullet: one of the identifiers matches the entry's
            // `idPattern`, which clause 5.2.8 only consults when no `id` is
            // registered beside it.
            axis = axis.add(
                Condition::all()
                    .add(pattern_only_entry())
                    .add(SeaExpr::exists(some_identifier_matches_registered_pattern(&ids))),
            );
        }
        if let Some(pattern) = self.id_pattern {
            // Fourth bullet: the asked-for pattern matches the entry's `id`.
            axis = axis.add(
                Condition::all()
                    .add(jsonb_exists(ent(), text_literal("id")))
                    .add(regex_match(registered_id(), SeaExpr::val(pattern.as_str()))),
            );
            // Fifth bullet: both sides carry a pattern; whether two regular
            // expressions admit a common identifier is not decidable cheaply,
            // so the entry is taken as matching.
            axis = axis.add(pattern_only_entry());
        }
        Some(axis)
    }
}

/// `ent->>'id'`.
fn registered_id() -> SimpleExpr {
    ent().json_get_text(text_literal("id"))
}

/// Whether the entry constrains the identifier by `idPattern` alone.
fn pattern_only_entry() -> SimpleExpr {
    Condition::all()
        .add(lacks_key(ent(), "id"))
        .add(jsonb_exists(ent(), text_literal("idPattern")))
        .into()
}

/// Whether the entry's `type` names `entity_type`.
///
/// Table 5.2.8-1 lets `type` be one name or an array of names, and the `?`
/// operator reads both shapes the same way: it is true when the text is the
/// scalar itself or one of the array's elements.
pub(crate) fn entity_type_admits(entity_type: &IriBuf) -> SimpleExpr {
    jsonb_exists(ent().json_get(text_literal("type")), SeaExpr::val(entity_type.as_str()))
}

/// `EXISTS (SELECT 1 FROM unnest(ids) AS queried WHERE queried ~ ent->>'idPattern')`.
fn some_identifier_matches_registered_pattern(ids: &[&str]) -> SelectStatement {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(unnest_of(text_array(ids)), Alias::new("queried"))
        .and_where(regex_match(
            SeaExpr::col((Alias::new("queried"), Alias::new("queried"))),
            ent().json_get_text(text_literal("idPattern")),
        ));
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn render(specification: &EntitySpecification<'_>) -> String {
        specification.matching_entities().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_type_selector_reads_the_entry_type_with_the_existence_operator() {
        // Table 5.2.8-1: `type` is a string or an array, and `?` admits both.
        let entity_type = iri("https://example.com/Vehicle");
        let sql = render(&EntitySpecification {
            entity_type: Some(&entity_type),
            ids: &[],
            id_pattern: None,
        });
        assert!(
            sql.contains("jsonb_exists(\"ent\".\"value\" -> ('type'), 'https://example.com/Vehicle')"),
            "{sql}"
        );
        assert!(!sql.contains("'id'"), "no identifier axis without identifiers: {sql}");
    }

    #[test]
    fn identifiers_admit_unconstrained_entries_exact_ids_and_pattern_only_entries_the_ids_match() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert!(
            sql.contains("(NOT jsonb_exists(\"ent\".\"value\", 'id')) AND (NOT jsonb_exists(\"ent\".\"value\", 'idPattern'))"),
            "{sql}"
        );
        assert!(
            sql.contains("(\"ent\".\"value\" ->> ('id')) = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2'])"),
            "{sql}"
        );
        assert!(
            sql.contains("UNNEST(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2']) AS \"queried\""),
            "{sql}"
        );
        assert!(sql.contains("\"queried\".\"queried\" ~ (\"ent\".\"value\" ->> ('idPattern'))"), "{sql}");
    }

    /// Clause 5.2.8: `id` takes precedence over `idPattern`, so an entry that
    /// registers both is only ever matched through its `id`.
    #[test]
    fn a_registered_pattern_is_consulted_only_on_entries_without_an_id() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert!(
            sql.contains("(NOT jsonb_exists(\"ent\".\"value\", 'id')) AND jsonb_exists(\"ent\".\"value\", 'idPattern') AND EXISTS"),
            "{sql}"
        );
    }

    #[test]
    fn a_queried_pattern_is_matched_against_registered_ids_and_admits_pattern_only_entries() {
        let pattern = Pattern::parse("urn:ngsi-ld:Fleet.*").expect("valid pattern");
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: Some(&pattern),
        });
        assert!(sql.contains("(\"ent\".\"value\" ->> ('id')) ~ 'urn:ngsi-ld:Fleet.*'"), "{sql}");
        assert!(
            sql.contains("(NOT jsonb_exists(\"ent\".\"value\", 'id')) AND jsonb_exists(\"ent\".\"value\", 'idPattern'))"),
            "the fifth bullet admits a pattern-only entry without evaluating it: {sql}"
        );
        assert!(!sql.contains("UNNEST"), "{sql}");
    }

    #[test]
    fn a_specification_without_any_axis_constrains_nothing() {
        let specification = EntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: None,
        };
        assert!(!specification.constrains());
        let sql = render(&specification);
        assert!(!sql.contains("WHERE"), "{sql}");
    }
}
