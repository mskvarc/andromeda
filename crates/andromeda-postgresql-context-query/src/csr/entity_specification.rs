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
    preselect::{entity_id_candidate, entity_id_pattern_candidate, entity_type_candidate},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    csr::expanded_entity_specification::ExpandedEntitySpecification,
    identifier_axis::IdentifierAxis,
    name_stage::Expanded,
    type_selection::TypeSelection,
};
use andromeda_postgresql_query::type_predicate::{names_condition, names_decide};
use andromeda_postgresql_schema::csr::RegistrationInformation;
use andromeda_postgresql_seaquery::{
    literal::int_literal,
    operator::regex_match,
    pg_array::{is_any_of, text_array},
    unnest::unnest_of,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement};

/// The condition a `registration_information` row meets when its `EntityInfo`
/// matches `specification`, or `None` when every row does.
///
/// Each axis carries its indexed test, and beside it the residual the test does
/// not decide exactly. The axes are conjuncts of one condition, so a caller
/// adding a further axis narrows the row whichever axes are present.
#[must_use]
pub fn row_matches(specification: &ExpandedEntitySpecification<'_>) -> Option<Condition> {
    let axes = [specification.entity_type.and_then(type_axis), identifier_axis(specification)];
    let row = axes.into_iter().flatten().fold(Condition::all(), Condition::add);
    (!row.is_empty()).then_some(row)
}

/// The clause 5.12 identifier conditions, or `None` when neither identifiers
/// nor a pattern were asked for.
fn identifier_axis(specification: &ExpandedEntitySpecification<'_>) -> Option<Condition> {
    match IdentifierAxis::of(specification.ids, specification.id_pattern) {
        IdentifierAxis::Any => None,
        IdentifierAxis::Ids(ids) => Some(Condition::all().add(entity_id_candidate(ids)).add(identifiers_admitted(ids))),
        // Fourth bullet: the asked-for pattern matches the row's `id`. First
        // and fifth: a row constraining the identifier not at all, or by a
        // pattern of its own, matches; whether two regular expressions admit a
        // common identifier is not decidable cheaply, so a registered pattern
        // is taken as matching. Both are the rows without an `id`.
        IdentifierAxis::Pattern(pattern) => Some(Condition::all().add(entity_id_pattern_candidate(pattern))),
    }
}

/// The type axis of a non-wildcard selection: the indexed overlap, and for a
/// selection that is not a disjunction of names, the residual on rows that
/// carry types. `None` when the selection admits every row.
fn type_axis(selection: &TypeSelection<Expanded>) -> Option<Condition> {
    let residual = names_condition(selection, &ri_col(RegistrationInformation::EntityTypes))?;
    let named = selection.named_types();
    if named.is_empty() {
        return None;
    }
    let axis = Condition::all().add(entity_type_candidate(&named));
    if names_decide(selection) {
        return Some(axis);
    }
    Some(axis.add(Condition::any().add(ri_col(RegistrationInformation::EntityTypes).is_null()).add(residual)))
}

/// Clause 5.12 second and third bullets, and the first: one of `ids` equals
/// the row's `id`, or the row has no `id` and either no pattern or a pattern
/// one of `ids` matches. Clause 5.2.8 consults a registered pattern only on a
/// row without an `id`.
fn identifiers_admitted(ids: &[Identifier]) -> Condition {
    let ids: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
    Condition::any()
        .add(is_any_of(ri_col(RegistrationInformation::EntityId), ids.iter().map(|id| (*id).to_owned())))
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
    use crate::csr::{entity_specification::row_matches, information_row::ri_col};
    use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, identifier::Identifier};
    use andromeda_ngsi_ld_query::{
        csr::expanded_entity_specification::ExpandedEntitySpecification,
        name_stage::Expanded,
        pattern::Pattern,
        type_selection::TypeSelection,
    };
    use andromeda_postgresql_schema::csr::RegistrationInformation;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, literal::int_literal};
    use sea_query::{ExprTrait, Query, SelectStatement};

    fn iri(value: &str) -> EntityTypeIri {
        value.parse().expect("valid IRI")
    }

    fn alternatives(shape: &[&[&str]]) -> TypeSelection<Expanded> {
        TypeSelection::of_alternatives(shape.iter().map(|conjunction| conjunction.iter().map(|value| iri(value)).collect()).collect())
            .expect("a non-empty shape")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(specification: &ExpandedEntitySpecification<'_>) -> SelectStatement {
        let row = row_matches(specification).expect("the specification constrains the entity axis");
        Query::select().expr(int_literal(1)).cond_where(row).take()
    }

    fn types(selection: &TypeSelection<Expanded>) -> SelectStatement {
        render(&ExpandedEntitySpecification {
            entity_type: Some(selection),
            ids: &[],
            id_pattern: None,
        })
    }

    /// Table 5.2.8-1: `type` is one name or several, both stored as an array,
    /// so one name is decided by the overlap with the wildcard sentinel.
    #[test]
    fn one_type_is_decided_by_the_indexed_overlap_alone() {
        let select = types(&alternatives(&[&["https://example.com/Vehicle"]]));
        assert_statement_snapshot!("entity-specification-one-type-is-decided-by-the-indexed-overlap-alone", select);
    }

    /// Clause 5.12 matches the selector against the row's types: a
    /// conjunction needs every type present, a disjunction one of them, and a
    /// row of an item without `entities` matches either.
    #[test]
    fn a_conjunction_needs_every_type_and_a_disjunction_one_of_them() {
        let conjunction = alternatives(&[&["https://example.com/Home", "https://example.com/Vehicle"]]);
        assert_statement_snapshot!(
            "entity-specification-a-conjunction-needs-every-type-and-a-disjunction-one-of-them-1",
            types(&conjunction)
        );
        let disjunction = alternatives(&[&["https://example.com/Home"], &["https://example.com/Vehicle"]]);
        assert_statement_snapshot!(
            "entity-specification-a-conjunction-needs-every-type-and-a-disjunction-one-of-them-2",
            types(&disjunction)
        );
    }

    #[test]
    fn the_wildcard_admits_every_row() {
        let selection = TypeSelection::Wildcard;
        let specification = ExpandedEntitySpecification {
            entity_type: Some(&selection),
            ids: &[],
            id_pattern: None,
        };
        assert!(row_matches(&specification).is_none());
    }

    #[test]
    fn identifiers_admit_unconstrained_rows_exact_ids_and_pattern_only_rows_the_ids_match() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let select = render(&ExpandedEntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert_statement_snapshot!(
            "entity-specification-identifiers-admit-unconstrained-rows-exact-ids-and-pattern-only-rows-the-ids-match",
            select
        );
    }

    /// Clause 5.2.8: `id` takes precedence over `idPattern`, so a row that
    /// registers both is only ever matched through its `id`.
    #[test]
    fn a_registered_pattern_is_consulted_only_on_rows_without_an_id() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let select = render(&ExpandedEntitySpecification {
            entity_type: None,
            ids: &ids,
            id_pattern: None,
        });
        assert_statement_snapshot!("entity-specification-a-registered-pattern-is-consulted-only-on-rows-without-an-id", select);
    }

    #[test]
    fn a_queried_pattern_is_matched_against_registered_ids_and_admits_rows_without_one() {
        let pattern = "urn:ngsi-ld:Fleet.*".parse::<Pattern>().expect("valid pattern");
        let select = render(&ExpandedEntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: Some(&pattern),
        });
        assert_statement_snapshot!(
            "entity-specification-a-queried-pattern-is-matched-against-registered-ids-and-admits-rows-without-one",
            select
        );
    }

    /// Clause 5.12: the identifier axis is one condition of the `EntityInfo`
    /// beside any other, so whatever is added to the row narrows it.
    #[test]
    fn a_pattern_without_a_type_is_a_conjunct_a_further_axis_narrows() {
        let pattern = "urn:ngsi-ld:Fleet.*".parse::<Pattern>().expect("valid pattern");
        let row = row_matches(&ExpandedEntitySpecification {
            entity_type: None,
            ids: &[],
            id_pattern: Some(&pattern),
        })
        .expect("a pattern constrains the entity axis")
        .add(ri_col(RegistrationInformation::PropertyNameKeys).is_not_null());
        let select = Query::select().expr(int_literal(1)).cond_where(row).take();
        assert_statement_snapshot!("entity-specification-a-pattern-without-a-type-is-a-conjunct-a-further-axis-narrows", select);
    }

    #[test]
    fn the_type_and_identifier_axes_are_one_row_condition() {
        let entity_type = alternatives(&[&["https://example.com/Vehicle"]]);
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let select = render(&ExpandedEntitySpecification {
            entity_type: Some(&entity_type),
            ids: &ids,
            id_pattern: None,
        });
        assert_statement_snapshot!("entity-specification-the-type-and-identifier-axes-are-one-row-condition", select);
    }

    #[test]
    fn a_specification_without_any_axis_constrains_nothing() {
        assert!(row_matches(&ExpandedEntitySpecification::default()).is_none());
    }
}
