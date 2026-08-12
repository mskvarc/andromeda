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
//! operations. Each dimension supersets in its own way:
//!
//! - **Entity type.** `entity_types` is the set of type names the selector's
//!   clause 4.17 expression mentions, or `NULL` where the expression admits
//!   every type. A `NULL` stands for "unconstrained" and is admitted; otherwise
//!   the requested type has to overlap the stored set. Overlap rather than
//!   containment is what the projection's widening of a conjunction into its
//!   members requires, and it is the operator a GIN index on the array serves.
//! - **Entity identifier.** A row constrains the identifier axis only when it
//!   carries `entity_id` or `entity_id_pattern`. An exact identifier is compared
//!   as text; a pattern row is admitted *pattern-agnostically* — the projection
//!   never evaluates `entity_id_pattern` as a regular expression in SQL, because
//!   PostgreSQL's regular-expression dialect is not the one clause 4.9 mandates
//!   and a dialect disagreement here would drop candidates rather than merely
//!   over-admit them.
//! - **Identifier pattern.** Discovery filters registrations whose declared
//!   `idPattern` equals the one asked for, so the projection compares the stored
//!   pattern text; a row that constrains no pattern is unconstrained and is
//!   admitted.
//! - **Attribute names.** Names are interned, so the comparison resolves the
//!   requested expanded IRIs through the `attribute_names` dictionary inside the
//!   same statement and overlaps the resulting key set with the row's two key
//!   arrays. A name absent from the dictionary resolves to no key and simply
//!   matches nothing, which is correct: no registration can have declared a name
//!   the dictionary never saw. A row whose two arrays are both `NULL` declared
//!   no names at all and is admitted unconditionally — the writer widens *both*
//!   arrays together when it cannot intern a declared name, so one array being
//!   `NULL` beside a populated one means the item genuinely declared nothing on
//!   that axis rather than that its keys were dropped.
//!
//! Dimensions are tested independently rather than against one row, because the
//! capabilities of a single information item are spread across sibling rows and
//! demanding one row satisfy two dimensions would exclude legitimate candidates.
//!
//! # Registrations with no projection rows
//!
//! A registration whose `information` is absent or empty constrains nothing and
//! serves everything, and so does one carrying an information item that
//! constrains no axis, since the items are disjunctive. Both project no rows at
//! all. Every predicate here is therefore disjoined with that emptiness test, so
//! an unrestricted registration stays a candidate on every dimension; reading
//! the projection as an inner join would drop exactly the registrations that
//! serve everything.
//!
//! # What the preselect does and does not buy
//!
//! That disjunction decides the shape of every plan this module can produce.
//! `NOT EXISTS (…) OR EXISTS (… AND dimension)` is a predicate *of the
//! registration*, and a registration that projects no rows satisfies it, so no
//! index over `registration_information` can enumerate the candidates: every
//! registration has to be examined, and discovery stays linear in the number of
//! registrations however selective the dimension is. Measurement confirms it —
//! `docs/proposals/database-schema-redesign-benchmarks.md` section 8 records
//! the emitted shape scanning the registration relation at 20,000 rows.
//!
//! What the projection and its selector indexes do buy is the cost of each
//! examination: the correlated probe answers from the routing rows instead of
//! unnesting the `information` document per registration, which is the work
//! this projection exists to remove. Making discovery sublinear in
//! registrations needs a different predicate shape — an indexable
//! "unconstrained" marker the emptiness test could be replaced by — and that is
//! migration-sequence step 8's measurement to take, not a change to make
//! silently here.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::pattern::Pattern;
use iri_rs::IriBuf;
use sea_query::{Expr as SeaExpr, SimpleExpr};

/// The correlated header shared by every projection subquery.
///
/// Correlation is written against the unaliased `context_source_registrations`
/// relation because both the discovery statement and the coverage statement
/// select from it under its own name.
const CORRELATED_PREFIX: &str = "SELECT 1 FROM \"registration_information\" ri \
     WHERE ri.\"registration_id\" = \"context_source_registrations\".\"id\"";

/// Whether the registration contributes no projection rows at all.
///
/// A registration that restricts no dimension projects nothing, so this test is
/// the open-registration escape hatch every dimension predicate disjoins with.
fn no_projection_rows() -> SimpleExpr {
    SeaExpr::cust(format!("NOT EXISTS ({CORRELATED_PREFIX})"))
}

/// Disjoin a dimension test with the open-registration escape hatch.
fn candidate(dimension: SimpleExpr) -> SimpleExpr {
    no_projection_rows().or(dimension)
}

/// Whether some projection row leaves `dimension_predicate` satisfiable.
///
/// `dimension_predicate` is the row-level SQL fragment, written against the `ri`
/// alias, and receives `values` as its own `$1`-based placeholders.
fn any_row<V, I>(dimension_predicate: &str, values: I) -> SimpleExpr
where
    V: Into<sea_query::Value>,
    I: IntoIterator<Item = V>,
{
    SeaExpr::cust_with_values(format!("EXISTS ({CORRELATED_PREFIX} AND ({dimension_predicate}))"), values)
}

/// Registrations that may declare `entity_type`, by the routing projection.
///
/// Rows that constrain no type are admitted, as is any row whose type set
/// contains the expanded type IRI the query was compiled against.
pub fn entity_type_candidate(entity_type: &IriBuf) -> SimpleExpr {
    // The requested type is wrapped by `ARRAY(SELECT ...)` rather than by an
    // array literal: sea-query's tokenizer reads `[` as a quoting delimiter, so
    // a placeholder written inside brackets is emitted verbatim and never bound.
    candidate(any_row(
        "ri.\"entity_types\" IS NULL OR ri.\"entity_types\" && ARRAY(SELECT $1::TEXT)",
        [entity_type.as_str().to_string()],
    ))
}

/// Registrations that may serve `entity_id`, by the routing projection.
///
/// A row that constrains neither identifier nor pattern is admitted, as is one
/// naming the identifier exactly. A pattern row is admitted whatever its
/// pattern: deciding it needs the clause-4.9 regular-expression engine, which is
/// the caller's residual evaluation, not PostgreSQL's.
pub fn entity_id_candidate(entity_id: &Uri) -> SimpleExpr {
    candidate(any_row(
        "(ri.\"entity_id\" IS NULL AND ri.\"entity_id_pattern\" IS NULL) \
         OR ri.\"entity_id\" = $1 \
         OR ri.\"entity_id_pattern\" IS NOT NULL",
        [entity_id.as_str().to_string()],
    ))
}

/// Registrations that may declare `id_pattern`, by the routing projection.
///
/// Discovery asks which registrations advertise a given `idPattern`, which is a
/// comparison of the pattern text itself rather than an evaluation of it, so the
/// stored pattern is compared as written; rows constraining no pattern are
/// admitted.
pub fn id_pattern_candidate(id_pattern: &Pattern) -> SimpleExpr {
    candidate(any_row(
        "ri.\"entity_id_pattern\" IS NULL OR ri.\"entity_id_pattern\" = $1",
        [id_pattern.as_str().to_string()],
    ))
}

/// Registrations that may declare one of `attribute_names`, by the projection.
///
/// The expanded property and relationship IRIs are resolved through the
/// `attribute_names` dictionary in the same statement, so no separate lookup
/// round trip is needed and an unknown name simply resolves to nothing. The
/// resolved keys are overlapped with each of the row's two key arrays — the
/// operator a GIN index on either array serves — and a row that declared no
/// names on either axis is admitted.
pub fn attribute_name_candidate(attribute_names: &[IriBuf]) -> SimpleExpr {
    let placeholders = (1..=attribute_names.len())
        .map(|position| format!("${position}"))
        .collect::<Vec<_>>()
        .join(", ");
    let requested_keys = format!("ARRAY(SELECT an.\"id\" FROM \"attribute_names\" an WHERE an.\"name\" IN ({placeholders}))::INTEGER[]");
    let names = attribute_names.iter().map(|name| name.as_str().to_string());
    candidate(any_row(
        &format!(
            "(ri.\"property_name_ids\" IS NULL AND ri.\"relationship_name_ids\" IS NULL) \
             OR ri.\"property_name_ids\" && {requested_keys} \
             OR ri.\"relationship_name_ids\" && {requested_keys}"
        ),
        names,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn render(predicate: SimpleExpr) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .and_where(predicate)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn every_dimension_keeps_a_registration_without_projection_rows_a_candidate() {
        // A registration that constrains nothing serves everything and projects
        // nothing, so each dimension has to disjoin the emptiness test.
        for sql in [
            render(entity_type_candidate(&iri("https://example.com/Vehicle"))),
            render(entity_id_candidate(&uri("urn:ngsi-ld:Vehicle:1"))),
            render(id_pattern_candidate(&Pattern::parse("urn:ngsi-ld:.*").expect("valid pattern"))),
            render(attribute_name_candidate(&[iri("https://example.com/speed")])),
        ] {
            assert!(sql.contains("NOT EXISTS (SELECT 1 FROM \"registration_information\""), "{sql}");
        }
    }

    #[test]
    fn every_dimension_correlates_on_the_registration_identifier() {
        let sql = render(entity_type_candidate(&iri("https://example.com/Vehicle")));
        assert!(sql.contains("ri.\"registration_id\" = \"context_source_registrations\".\"id\""), "{sql}");
    }

    #[test]
    fn the_type_dimension_overlaps_the_stored_type_set_and_admits_rows_that_constrain_no_type() {
        let sql = render(entity_type_candidate(&iri("https://example.com/Vehicle")));
        assert!(sql.contains("ri.\"entity_types\" IS NULL OR ri.\"entity_types\" && ARRAY(SELECT "), "{sql}");
        assert!(sql.contains("'https://example.com/Vehicle'"), "{sql}");
    }

    #[test]
    fn the_identifier_dimension_admits_pattern_rows_without_evaluating_the_pattern() {
        // Clause 4.9 regular expressions are decided by the residual, never by
        // PostgreSQL: a pattern row is a candidate whatever it holds.
        let sql = render(entity_id_candidate(&uri("urn:ngsi-ld:Vehicle:1")));
        assert!(sql.contains("ri.\"entity_id_pattern\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains(" ~ "), "the preselect must not run a regular expression: {sql}");
    }

    #[test]
    fn the_identifier_dimension_admits_rows_that_constrain_neither_identifier_nor_pattern() {
        let sql = render(entity_id_candidate(&uri("urn:ngsi-ld:Vehicle:1")));
        assert!(sql.contains("(ri.\"entity_id\" IS NULL AND ri.\"entity_id_pattern\" IS NULL)"), "{sql}");
    }

    #[test]
    fn the_pattern_dimension_compares_the_pattern_text_rather_than_evaluating_it() {
        let sql = render(id_pattern_candidate(&Pattern::parse("urn:ngsi-ld:.*").expect("valid pattern")));
        assert!(sql.contains("ri.\"entity_id_pattern\" IS NULL OR ri.\"entity_id_pattern\" = "), "{sql}");
        assert!(sql.contains("'urn:ngsi-ld:.*'"), "{sql}");
    }

    #[test]
    fn the_attribute_dimension_resolves_names_through_the_dictionary_in_one_statement() {
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed"), iri("https://example.com/temp")]));
        assert!(
            sql.contains("ARRAY(SELECT an.\"id\" FROM \"attribute_names\" an WHERE an.\"name\" IN ("),
            "{sql}"
        );
        assert!(sql.contains("'https://example.com/speed'"), "{sql}");
        assert!(sql.contains("'https://example.com/temp'"), "{sql}");
    }

    #[test]
    fn the_attribute_dimension_overlaps_both_key_arrays_and_admits_rows_declaring_no_name() {
        // A property name and a relationship name are declared on separate
        // members of the same information item, so both arrays are tested; a row
        // whose two arrays are `NULL` declared nothing and stays a candidate.
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed")]));
        assert!(
            sql.contains("(ri.\"property_name_ids\" IS NULL AND ri.\"relationship_name_ids\" IS NULL)"),
            "{sql}"
        );
        assert!(sql.contains("ri.\"property_name_ids\" && ARRAY("), "{sql}");
        assert!(sql.contains("ri.\"relationship_name_ids\" && ARRAY("), "{sql}");
    }
}
