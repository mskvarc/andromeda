//! The clause 5.12 entity specification, rendered against one
//! `registration_information` row.
//!
//! A discovery query's type selector, entity identifiers and identifier pattern
//! match an `EntityInfo` when the type selector matches the entry's type(s)
//! and one of the five clause 5.12 identifier conditions holds. A row of an
//! item without `entities` matches every specification. Clause 5.2.8 adds that
//! `id` takes precedence over `idPattern`, so a row carrying both is matched
//! through its `id` alone.
//!
//! Neither side's pattern text reaches PostgreSQL's regular-expression engine:
//! a queried `idPattern` binds its portable restatement, and a registered one
//! is evaluated as the restatement stored beside it
//! (`entity_id_pattern_form`), so both read `(?i)` and `(?m)` as the broker's
//! in-memory engine does (clause 4.9).

use crate::csr::{
    information_row::ri_col,
    preselect::{entity_id_candidate, entity_type_candidate},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::grammar::TypeExpr};
use andromeda_postgresql_schema::csr::RegistrationInformation;
use andromeda_postgresql_seaquery::{
    literal::int_literal,
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
    /// The expanded clause 4.17 type selector, if any.
    pub entity_type: Option<&'query TypeExpr>,
    /// The entity identifiers asked for; empty when none were.
    pub ids: &'query [Identifier],
    /// The identifier pattern asked for, if any.
    pub id_pattern: Option<&'query Pattern>,
}

impl EntitySpecification<'_> {
    /// Whether the specification constrains the entity axis at all.
    #[must_use]
    pub const fn constrains(&self) -> bool {
        self.entity_type.is_some() || !self.ids.is_empty() || self.id_pattern.is_some()
    }

    /// The condition a `registration_information` row meets when its
    /// `EntityInfo` matches the specification, or `None` when every row does.
    ///
    /// Each axis carries its indexed test, and beside it the residual the test
    /// does not decide exactly.
    #[must_use]
    pub fn row_matches(&self) -> Option<Condition> {
        let axes = [self.entity_type.and_then(type_axis), self.identifier_axis()];
        axes.into_iter().flatten().reduce(Condition::add)
    }

    /// The clause 5.12 identifier conditions, or `None` when neither
    /// identifiers nor a pattern were asked for.
    fn identifier_axis(&self) -> Option<Condition> {
        match IdentifierAxis::of(self.ids, self.id_pattern) {
            IdentifierAxis::Any => None,
            IdentifierAxis::Ids(ids) => Some(Condition::all().add(entity_id_candidate(ids)).add(identifiers_admitted(ids))),
            // Fourth bullet: the asked-for pattern matches the row's `id`.
            // First and fifth: a row constraining the identifier not at all,
            // or by a pattern of its own, matches; whether two regular
            // expressions admit a common identifier is not decidable cheaply,
            // so a registered pattern is taken as matching. Both are the rows
            // without an `id`.
            IdentifierAxis::Pattern(pattern) => Some(Condition::any().add(ri_col(RegistrationInformation::EntityId).is_null()).add(regex_match(
                ri_col(RegistrationInformation::EntityId),
                SeaExpr::val(pattern.portable().as_str()),
            ))),
        }
    }
}

/// The type axis of a non-wildcard selection: the indexed overlap, and for a
/// selection that is not a disjunction of names, the residual on rows that
/// carry types. `None` when the selection admits every row.
fn type_axis(selection: &TypeExpr) -> Option<Condition> {
    let residual = types_satisfy(selection)?;
    let named = selection.named_types();
    if named.is_empty() {
        return None;
    }
    let axis = Condition::all().add(entity_type_candidate(&named));
    if is_disjunction_of_names(selection) {
        return Some(axis);
    }
    Some(axis.add(Condition::any().add(ri_col(RegistrationInformation::EntityTypes).is_null()).add(residual)))
}

/// Whether the overlap with the selection's names decides it exactly: one
/// name, or a disjunction of names.
fn is_disjunction_of_names(selection: &TypeExpr) -> bool {
    match selection {
        TypeExpr::Type(_) => true,
        TypeExpr::Or(children) => children.iter().all(|child| matches!(child, TypeExpr::Type(_))),
        TypeExpr::Wildcard | TypeExpr::And(_) => false,
    }
}

/// Whether the row's `entity_types` satisfy the clause 4.17 `selection`, or
/// `None` when the selection admits every row (the wildcard).
///
/// Clause 5.12 matches the type selector against the entity types of the
/// `EntityInfo`: a conjunction needs every named type among them and a
/// disjunction one of them, so the names a conjunction lists directly are one
/// `@>` and those a disjunction lists one `&&`.
fn types_satisfy(selection: &TypeExpr) -> Option<SimpleExpr> {
    match selection {
        TypeExpr::Wildcard => None,
        TypeExpr::Type(entity_type) => Some(entity_types_hold(entity_type)),
        TypeExpr::And(children) => {
            let names = direct_names(children);
            let all = children
                .iter()
                .filter(|child| !matches!(child, TypeExpr::Type(_)))
                .filter_map(types_satisfy)
                .fold(Condition::all(), Condition::add);
            let all = if names.is_empty() {
                all
            } else {
                all.add(ri_col(RegistrationInformation::EntityTypes).array_contains(text_array(&names)))
            };
            Some(all.into())
        }
        TypeExpr::Or(children) => {
            let names = direct_names(children);
            let others = children
                .iter()
                .filter(|child| !matches!(child, TypeExpr::Type(_)))
                .map(types_satisfy)
                .collect::<Option<Vec<_>>>()?;
            let any = others.into_iter().fold(Condition::any(), Condition::add);
            let any = if names.is_empty() {
                any
            } else {
                any.add(ri_col(RegistrationInformation::EntityTypes).array_overlaps(text_array(&names)))
            };
            Some(any.into())
        }
    }
}

/// `ri.entity_types @> ARRAY[entity_type]`.
fn entity_types_hold(entity_type: &IriBuf) -> SimpleExpr {
    ri_col(RegistrationInformation::EntityTypes).array_contains(text_array(&[entity_type.as_str()]))
}

/// The names `children` list directly, each a leaf of the expression.
fn direct_names(children: &[TypeExpr]) -> Vec<&str> {
    children
        .iter()
        .filter_map(|child| match child {
            TypeExpr::Type(entity_type) => Some(entity_type.as_str()),
            TypeExpr::Wildcard | TypeExpr::And(_) | TypeExpr::Or(_) => None,
        })
        .collect()
}

/// Clause 5.12 second and third bullets, and the first: one of `ids` equals
/// the row's `id`, or the row has no `id` and either no pattern or a pattern
/// one of `ids` matches. Clause 5.2.8 consults a registered pattern only on a
/// row without an `id`.
fn identifiers_admitted(ids: &[Identifier]) -> Condition {
    let ids: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
    Condition::any()
        .add(SeaExpr::expr(ri_col(RegistrationInformation::EntityId)).eq(any(text_array(&ids))))
        .add(
            Condition::all().add(ri_col(RegistrationInformation::EntityId).is_null()).add(
                Condition::any()
                    .add(ri_col(RegistrationInformation::EntityIdPatternForm).is_null())
                    .add(SeaExpr::exists(some_identifier_matches_registered_pattern(&ids))),
            ),
        )
}

/// `SELECT 1 FROM unnest(ids) AS queried WHERE queried ~ ri.entity_id_pattern_form`.
fn some_identifier_matches_registered_pattern(ids: &[&str]) -> SelectStatement {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(unnest_of(text_array(ids)), Alias::new("queried"))
        .and_where(regex_match(
            SeaExpr::col((Alias::new("queried"), Alias::new("queried"))),
            ri_col(RegistrationInformation::EntityIdPatternForm),
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

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(specification: &EntitySpecification<'_>) -> String {
        let row = specification.row_matches().expect("the specification constrains the entity axis");
        Query::select().expr(int_literal(1)).cond_where(row).to_string(PostgresQueryBuilder)
    }

    fn types(selection: &TypeExpr) -> String {
        render(&EntitySpecification {
            entity_type: Some(selection),
            ids: &[],
            id_pattern: None,
        })
    }

    /// Table 5.2.8-1: `type` is one name or several, both stored as an array,
    /// so one name is decided by the overlap with the wildcard sentinel.
    #[test]
    fn one_type_is_decided_by_the_indexed_overlap_alone() {
        let sql = types(&TypeExpr::Type(iri("https://example.com/Vehicle")));
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Vehicle','*']"),
            "{sql}"
        );
        assert!(!sql.contains("@>"), "{sql}");
        assert!(!sql.contains("entity_id"), "no identifier axis without identifiers: {sql}");
    }

    /// Clause 5.12 matches the selector against the row's types: a
    /// conjunction needs every type present, a disjunction one of them, and a
    /// row of an item without `entities` matches either.
    #[test]
    fn a_conjunction_needs_every_type_and_a_disjunction_one_of_them() {
        let conjunction = TypeExpr::And(vec![
            TypeExpr::Type(iri("https://example.com/Home")),
            TypeExpr::Type(iri("https://example.com/Vehicle")),
        ]);
        let sql = types(&conjunction);
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Home','https://example.com/Vehicle','*']"),
            "{sql}"
        );
        assert!(
            sql.contains("\"ri\".\"entity_types\" IS NULL OR (\"ri\".\"entity_types\" @> ARRAY ['https://example.com/Home','https://example.com/Vehicle'])"),
            "{sql}"
        );
        let disjunction = TypeExpr::Or(vec![
            TypeExpr::Type(iri("https://example.com/Home")),
            TypeExpr::Type(iri("https://example.com/Vehicle")),
        ]);
        let sql = types(&disjunction);
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Home','https://example.com/Vehicle','*']"),
            "{sql}"
        );
        assert!(!sql.contains("@>"), "a disjunction of names is decided by the overlap: {sql}");
    }

    #[test]
    fn a_nested_selection_keeps_its_boolean_structure_in_the_residual() {
        let nested = TypeExpr::And(vec![
            TypeExpr::Type(iri("https://example.com/Home")),
            TypeExpr::Or(vec![
                TypeExpr::Type(iri("https://example.com/Vehicle")),
                TypeExpr::Type(iri("https://example.com/Boat")),
            ]),
        ]);
        let sql = types(&nested);
        assert!(
            sql.contains("\"ri\".\"entity_types\" && ARRAY ['https://example.com/Vehicle','https://example.com/Boat']"),
            "{sql}"
        );
        assert!(sql.contains("\"ri\".\"entity_types\" @> ARRAY ['https://example.com/Home']"), "{sql}");
    }

    #[test]
    fn a_disjunction_with_the_wildcard_admits_every_row() {
        let selection = TypeExpr::Or(vec![TypeExpr::Type(iri("https://example.com/Home")), TypeExpr::Wildcard]);
        let specification = EntitySpecification {
            entity_type: Some(&selection),
            ids: &[],
            id_pattern: None,
        };
        assert!(specification.row_matches().is_none());
    }

    #[test]
    fn identifiers_admit_unconstrained_rows_exact_ids_and_pattern_only_rows_the_ids_match() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') IN ('urn:ngsi-ld:Vehicle:1', 'urn:ngsi-ld:Vehicle:2', '')"),
            "{sql}"
        );
        assert!(
            sql.contains("\"ri\".\"entity_id\" = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2'])"),
            "{sql}"
        );
        assert!(
            sql.contains("\"queried\".\"queried\" ~ \"ri\".\"entity_id_pattern_form\""),
            "the registered pattern is evaluated as its stored restatement: {sql}"
        );
    }

    /// Clause 5.2.8: `id` takes precedence over `idPattern`, so a row that
    /// registers both is only ever matched through its `id`.
    #[test]
    fn a_registered_pattern_is_consulted_only_on_rows_without_an_id() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert!(
            sql.contains("\"ri\".\"entity_id\" IS NULL AND (\"ri\".\"entity_id_pattern_form\" IS NULL OR EXISTS(SELECT 1 FROM UNNEST(ARRAY ['urn:ngsi-ld:Vehicle:1']) AS \"queried\" WHERE \"queried\".\"queried\" ~ \"ri\".\"entity_id_pattern_form\"))"),
            "{sql}"
        );
    }

    #[test]
    fn a_queried_pattern_is_matched_against_registered_ids_and_admits_rows_without_one() {
        let pattern = Pattern::parse("urn:ngsi-ld:Fleet.*").expect("valid pattern");
        let sql = render(&EntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: Some(&pattern),
        });
        assert!(
            sql.contains("\"ri\".\"entity_id\" IS NULL OR (\"ri\".\"entity_id\" ~ 'urn:ngsi-ld:Fleet.*')"),
            "{sql}"
        );
        assert!(!sql.contains("UNNEST"), "{sql}");
    }

    #[test]
    fn the_type_and_identifier_axes_are_one_row_condition() {
        let entity_type = TypeExpr::Type(iri("https://example.com/Vehicle"));
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let sql = render(&EntitySpecification {
            entity_type: Some(&entity_type),
            ids: &ids,
            id_pattern: None,
        });
        assert!(sql.contains("\"entity_types\""), "{sql}");
        assert!(sql.contains("\"entity_id\""), "{sql}");
    }

    #[test]
    fn a_specification_without_any_axis_constrains_nothing() {
        let specification = EntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: None,
        };
        assert!(!specification.constrains());
        assert!(specification.row_matches().is_none());
    }
}
