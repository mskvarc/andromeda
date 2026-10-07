//! Candidate preselection over the `registration_information` routing projection.
//!
//! A registration's `information` (clause 5.2.9) is a nested document, and every
//! predicate that reads it — discovery (clause 5.9) and the forwarding coverage
//! decision (clause 5.10.2) alike — has to unnest it per row. The routing
//! projection holds the same selectors flattened, one row per information item
//! per entity selector, so those predicates can be preceded by an indexed
//! semi-join that discards registrations no unnesting could ever have matched.
//!
//! # Superset discipline
//!
//! Every predicate in this module is a *superset* filter: it must admit every
//! registration the authoritative evaluation admits, and it is free to admit
//! more. Callers keep their existing evaluation as the oracle and merely
//! conjoin a preselect, so a false positive costs one wasted residual test while
//! a false negative silently removes a context source from distributed
//! operations.
//!
//! # Sentinels instead of NULLs
//!
//! An axis a row does not constrain is stored as a sentinel, never as `NULL`:
//! `{'*'}` for the type set, `''` for the identifier, `{0}` for the two
//! attribute-key arrays. A registration that constrains nothing at all is one
//! wildcard row carrying every sentinel. That is what lets each dimension be a
//! *single* predicate that names the sentinel beside the requested value —
//! `entity_types && ARRAY[$1, '*']`, `entity_id IN ($1, '')`,
//! `property_name_keys && array_append(ARRAY(<keys>), 0)` — with one index
//! behind it, and it removes any emptiness test: a `NOT EXISTS (SELECT 1 FROM
//! registration_information …)` is a predicate *of the registration*, so the
//! candidate set could not be enumerated from the projection alone and
//! discovery would stay linear in the number of registrations however
//! selective the dimension. With sentinel rows the semi-join is driven from the
//! projection's own indexes, and a selective type or attribute name reaches the
//! registrations that declare it (or declare everything) without reading the
//! rest.
//!
//! Each dimension supersets in its own way:
//!
//! - **Entity type.** `entity_types` is the set of type names the selector's
//!   clause 4.17 expression mentions, or `{'*'}` where the expression admits
//!   every type. Overlap rather than containment is what the projection's
//!   widening of a conjunction into its members requires, and it is the
//!   operator a GIN index on the array serves.
//! - **Entity identifier.** A row constrains the identifier axis only when it
//!   carries an exact `entity_id` or an `entity_id_pattern`. An exact identifier
//!   is compared as text beside the sentinel; a pattern row is admitted
//!   *pattern-agnostically* — the projection never evaluates `entity_id_pattern`
//!   as a regular expression in SQL, because PostgreSQL's dialect is not the one
//!   clause 4.9 mandates and a dialect disagreement here would drop candidates
//!   rather than merely over-admit them.
//! - **Identifier pattern.** A queried `idPattern` is not preselected on: clause
//!   5.12 lets it match a registered identifier (a regular-expression
//!   evaluation), any registered pattern, and any entry constraining neither,
//!   which together admit every row, so the residual unnesting decides it alone.
//! - **Attribute names.** Names are interned, so the comparison resolves the
//!   requested expanded IRIs through the `attribute_names` dictionary inside the
//!   same statement and overlaps the resulting key set, with the sentinel
//!   appended, against the row's two key arrays. A name absent from the
//!   dictionary resolves to no key and simply matches nothing, which is correct:
//!   no registration can have declared a name the dictionary never saw. A row
//!   whose two arrays are both `{0}` declared no names at all and is admitted by
//!   the sentinel; a row that declared names on one axis only stores `{}` on the
//!   other, which overlaps nothing.
//!
//! Dimensions are tested independently rather than against one row, because the
//! capabilities of a single information item are spread across sibling rows and
//! demanding one row satisfy two dimensions would exclude legitimate candidates.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::{
    csr::{Csr, RegistrationInformation},
    name_dictionary::AttributeNames,
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::int_literal,
    pg_array::{any, array_append, array_of_subquery, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use iri_rs::IriBuf;
use sea_query::{Alias, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The type-axis sentinel: the row admits every type.
const ANY_TYPE: &str = "*";

/// The identifier-axis sentinel: the row names no exact identifier.
const ANY_IDENTIFIER: &str = "";

/// The attribute-axis sentinel: the row declared no name on either axis.
const ANY_ATTRIBUTE_KEY: i32 = 0;

/// The alias every projection subquery reads `registration_information` under.
fn ri() -> Alias {
    Alias::new("ri")
}

/// A `"ri"."column"` reference into the correlated projection scan.
fn ri_col(column: RegistrationInformation) -> SimpleExpr {
    SeaExpr::col((ri(), column))
}

/// The correlated scan shared by every projection subquery, narrowed by one
/// dimension predicate.
///
/// Correlation is written against the unaliased `context_source_registrations`
/// relation because both the discovery statement and the coverage statement
/// select from it under its own name.
fn correlated_scan(dimension: SimpleExpr) -> SelectStatement {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_as(RegistrationInformation::Table, ri())
        .and_where(SeaExpr::col((ri(), RegistrationInformation::RegistrationId)).eq(SeaExpr::col((Csr::Table, Csr::Id))))
        .and_where(dimension);
    scan
}

/// Whether some projection row of the registration satisfies a dimension.
fn any_row(dimension: SimpleExpr) -> SimpleExpr {
    SeaExpr::exists(correlated_scan(dimension))
}

/// Registrations that may declare `entity_type`, by the routing projection.
///
/// One overlap against the requested type IRI and the wildcard sentinel, which
/// is what `idx_registration_information_entity_types` serves. The two values
/// cross the bind boundary as one `text[]`.
#[must_use]
pub fn entity_type_candidate(entity_type: &IriBuf) -> SimpleExpr {
    any_row(ri_col(RegistrationInformation::EntityTypes).array_overlaps(text_array(&[entity_type.as_str(), ANY_TYPE])))
}

/// Registrations that may serve one of `entity_ids`, by the routing projection.
///
/// A row naming one of the identifiers exactly or naming none (the `''`
/// sentinel) is admitted through `idx_registration_information_entity_id`; a
/// pattern row is admitted whatever its pattern through the partial
/// `idx_registration_information_entity_id_pattern`, because deciding it needs
/// the clause-4.9 regular-expression engine, which is the caller's residual
/// evaluation, not the preselect's.
pub fn entity_id_candidate(entity_ids: &[Identifier]) -> SimpleExpr {
    let named = entity_ids.iter().map(Identifier::as_str).chain([ANY_IDENTIFIER]);
    any_row(
        ri_col(RegistrationInformation::EntityId)
            .is_in(named)
            .or(ri_col(RegistrationInformation::EntityIdPattern).is_not_null()),
    )
}

/// Registrations that may declare one of `attribute_names`, by the projection.
///
/// The expanded property and relationship IRIs are resolved through the
/// `attribute_names` dictionary in the same statement, so no separate lookup
/// round trip is needed and an unknown name simply resolves to nothing. The
/// resolved keys, with the wildcard sentinel appended, are overlapped with each
/// of the row's two key arrays — the operator the two GIN indexes serve, one
/// per array — so a row declaring one of the names on either axis and a row
/// declaring no name at all are both admitted by one indexed test each.
///
/// The dictionary resolution appears in both overlaps and therefore binds the
/// requested names twice. That is the price of two indexable operands and it is
/// small: the resolution is an index lookup per name against a dictionary the
/// planner treats as a constant array for the overlap.
#[must_use]
pub fn attribute_name_candidate(attribute_names: &[IriBuf]) -> SimpleExpr {
    // Each overlap resolves the dictionary independently, so the keys
    // subquery is rebuilt — and its names rebound — per operand.
    let requested_keys = || {
        let names: Vec<&str> = attribute_names.iter().map(iri_rs::Iri::as_str).collect();
        let mut dictionary = Query::select();
        dictionary
            .expr(SeaExpr::col((Alias::new("an"), AttributeNames::AttributeNameKey)))
            .from_as(AttributeNames::Table, Alias::new("an"))
            // One bound `text[]` rather than a placeholder per name, so a large
            // requested-attribute set cannot exhaust the parameter ceiling.
            .and_where(SeaExpr::col((Alias::new("an"), AttributeNames::Name)).eq(any(text_array(&names))));
        array_append(cast(array_of_subquery(dictionary), PostgresType::IntegerArray), SeaExpr::val(ANY_ATTRIBUTE_KEY))
    };
    any_row(
        ri_col(RegistrationInformation::PropertyNameKeys)
            .array_overlaps(requested_keys())
            .or(ri_col(RegistrationInformation::RelationshipNameKeys).array_overlaps(requested_keys())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(predicate: SimpleExpr) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .and_where(predicate)
            .to_string(PostgresQueryBuilder)
    }

    fn every_dimension() -> [String; 3] {
        [
            render(entity_type_candidate(&iri("https://example.com/Vehicle"))),
            render(entity_id_candidate(&[uri("urn:ngsi-ld:Vehicle:1")])),
            render(attribute_name_candidate(&[iri("https://example.com/speed")])),
        ]
    }

    /// The correlation every subquery carries.
    const CORRELATION: &str = "\"ri\".\"registration_id\" = \"context_source_registrations\".\"id\"";

    #[test]
    fn no_dimension_tests_for_the_absence_of_projection_rows() {
        // A registration that constrains nothing is one wildcard row, so no
        // dimension needs an emptiness escape hatch.
        for sql in every_dimension() {
            assert!(!sql.contains("NOT EXISTS"), "the emptiness test must be gone: {sql}");
            assert!(!sql.contains("IS NULL OR"), "no dimension disjoins a NULL test: {sql}");
        }
    }

    #[test]
    fn every_dimension_is_one_correlated_subquery_over_the_projection() {
        for sql in every_dimension() {
            pretty_assertions::assert_eq!(sql.matches("\"registration_information\"").count(), 1, "{sql}");
            pretty_assertions::assert_eq!(sql.matches(CORRELATION).count(), 1, "{sql}");
        }
    }

    #[test]
    fn the_type_dimension_overlaps_the_requested_type_beside_the_wildcard_sentinel() {
        let (sql, values) = Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .and_where(entity_type_candidate(&iri("https://example.com/Vehicle")))
            .build(PostgresQueryBuilder);
        assert!(sql.contains("\"ri\".\"entity_types\" && $1"), "{sql}");
        pretty_assertions::assert_eq!(values.0.len(), 1, "the type and the sentinel cross as one array: {sql}");
        let rendered = render(entity_type_candidate(&iri("https://example.com/Vehicle")));
        assert!(rendered.contains("ARRAY ['https://example.com/Vehicle','*']"), "{rendered}");
        assert!(!rendered.contains("IS NULL"), "{rendered}");
    }

    #[test]
    fn the_identifier_dimension_names_the_identifier_beside_the_sentinel_and_admits_pattern_rows_unevaluated() {
        // Clause 4.9 regular expressions are decided by the residual, never by
        // PostgreSQL: a pattern row is a candidate whatever it holds.
        let sql = render(entity_id_candidate(&[uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")]));
        assert!(
            sql.contains("\"ri\".\"entity_id\" IN ('urn:ngsi-ld:Vehicle:1', 'urn:ngsi-ld:Vehicle:2', '')"),
            "{sql}"
        );
        assert!(sql.contains("OR \"ri\".\"entity_id_pattern\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains(" ~ "), "the preselect must not run a regular expression: {sql}");
    }

    #[test]
    fn the_attribute_dimension_resolves_names_through_the_dictionary_and_appends_the_sentinel() {
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed"), iri("https://example.com/temp")]));
        assert!(sql.contains("\"attribute_names\""), "{sql}");
        assert!(
            sql.contains("\"an\".\"name\" = ANY("),
            "the requested names bind as one array, not a placeholder per name: {sql}"
        );
        assert!(sql.contains("'https://example.com/speed'"), "{sql}");
        assert!(sql.contains("'https://example.com/temp'"), "{sql}");
        assert!(sql.contains("\"ri\".\"property_name_keys\" && array_append(ARRAY((SELECT"), "{sql}");
        assert!(sql.contains("\"ri\".\"relationship_name_keys\" && array_append(ARRAY((SELECT"), "{sql}");
        assert!(sql.contains("::integer[], 0)"), "the sentinel is appended to the resolved keys: {sql}");
        assert!(!sql.contains("IS NULL"), "{sql}");
    }

    #[test]
    fn the_attribute_dimension_binds_the_requested_names_once_per_overlap() {
        // Both overlaps resolve the dictionary independently, so each requested
        // name appears once per operand and none is left unbound.
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed")]));
        pretty_assertions::assert_eq!(sql.matches("'https://example.com/speed'").count(), 2, "{sql}");
        assert!(!sql.contains('$'), "every placeholder must be bound: {sql}");
    }
}
