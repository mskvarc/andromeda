//! The indexed tests over `registration_information` rows, one per axis.
//!
//! Absence is `NULL` in the rows; the indexes spell an absent axis as a
//! sentinel, and each test here names the sentinel beside the requested
//! values through the very expression the index is built on, so one index
//! serves one test:
//!
//! - **Entity type.** `coalesce(entity_types, '{*}')` overlaps the requested
//!   names with `*` appended: a row of an item without `entities` admits every
//!   type (clause 5.12: "If there is no `EntityInfo` specified in the
//!   `RegistrationInfo`, the entity specification is considered matching").
//!   For one name or a disjunction of names the overlap is exact; a clause 4.17
//!   conjunction needs its residual (`@>`) beside it.
//! - **Entity identifier.** `coalesce(entity_id, '')` is one of the requested
//!   identifiers or `''`, or the row carries a pattern. A pattern row is
//!   admitted whatever its pattern; the residual evaluates it.
//! - **Entity identifier pattern.** `coalesce(entity_id, '')` is `''`, or
//!   `entity_id` matches the requested pattern through its trigram index. The
//!   test is exact: a row without an `id` constrains the identifier not at all
//!   or by a pattern of its own, and clause 5.12 takes either as matching.
//! - **Attribute names.** The requested names resolve to their
//!   `attribute_names` keys in the statement, and the keys overlap either axis
//!   as stored, or the row's item declares no name on either axis. Each branch
//!   is one index of its own, the last a partial index whose predicate is the
//!   branch. A row matches when its item declares no name or declares one of the
//!   requested names on either axis (clause 5.12). An axis the item leaves out
//!   is `NULL` and overlaps nothing. A name the dictionary never saw resolves to
//!   no key and matches only rows declaring no name.

use crate::csr::information_row::ri_col;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{filter::predicate::PatternPolarity, pattern::Pattern};
use andromeda_postgresql_query::operator::{PatternIndexes, pattern as pattern_predicate};
use andromeda_postgresql_schema::{csr::RegistrationInformation, name_dictionary::AttributeNames};
use andromeda_postgresql_seaquery::{
    literal::text_literal,
    pg_array::{array_of_subquery, is_any_of, text_array},
    pg_operator::PgBinaryOperator,
};
use iri_rs::IriBuf;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Func, Iden, Query, SimpleExpr};
use std::fmt;

/// The type-axis sentinel: the row's item has no `entities` and admits every
/// type. It names no type.
pub const ANY_TYPE: &str = "*";

/// The identifier-axis sentinel: the row names no exact identifier.
const ANY_IDENTIFIER: &str = "";

/// `coalesce(ri.entity_types, '{*}')`, the expression
/// `idx_registration_information_entity_types` indexes.
#[must_use]
pub fn entity_types_or_wildcard() -> SimpleExpr {
    Func::coalesce([ri_col(RegistrationInformation::EntityTypes), text_literal(&format!("{{{ANY_TYPE}}}"))]).into()
}

/// `coalesce(ri.entity_id, '')`, the expression
/// `idx_registration_information_entity_id` indexes.
fn entity_id_or_none() -> SimpleExpr {
    Func::coalesce([ri_col(RegistrationInformation::EntityId), text_literal(ANY_IDENTIFIER)]).into()
}

/// Whether the row's item declares no name on either axis, the predicate of
/// `idx_registration_information_undeclared_names`.
fn declares_no_name() -> Condition {
    Condition::all()
        .add(ri_col(RegistrationInformation::PropertyNameKeys).is_null())
        .add(ri_col(RegistrationInformation::RelationshipNameKeys).is_null())
}

/// Whether the row may admit one of `entity_types`: its types overlap them, or
/// its item has no `entities`. The values cross the bind boundary as one
/// `text[]`.
#[must_use]
pub fn entity_type_candidate(entity_types: &[IriBuf]) -> SimpleExpr {
    let named: Vec<&str> = entity_types.iter().map(IriBuf::as_str).chain([ANY_TYPE]).collect();
    entity_types_or_wildcard().array_overlaps(text_array(&named))
}

/// Whether the row may admit one of `entity_ids`: it names one of them, names
/// none, or carries a pattern, which the caller's residual evaluates. The
/// values cross the bind boundary as one `text[]`.
#[must_use]
pub fn entity_id_candidate(entity_ids: &[Identifier]) -> SimpleExpr {
    let named = entity_ids.iter().map(Identifier::as_str).chain([ANY_IDENTIFIER]).map(str::to_owned);
    is_any_of(entity_id_or_none(), named).or(ri_col(RegistrationInformation::EntityIdPattern).is_not_null())
}

/// Whether the row admits `pattern`: it names no `id`, or its `id` matches the
/// pattern's portable restatement.
///
/// The `id`-less branch is spelled as the identifier sentinel, so the B-tree
/// the identifier test reads serves it and a `BitmapOr` with the trigram index
/// can drive the whole disjunction.
#[must_use]
pub fn entity_id_pattern_candidate(pattern: &Pattern) -> SimpleExpr {
    entity_id_or_none().eq(text_literal(ANY_IDENTIFIER)).or(pattern_predicate(
        ri_col(RegistrationInformation::EntityId),
        PatternPolarity::Match,
        pattern,
        PatternIndexes::Trigrams,
    ))
}

/// The alias of one requested attribute name's dictionary row, `an`.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct RequestedName;

/// Whether the row's item declares one of `attribute_names`, or no name at all
/// (clause 5.12).
///
/// The names resolve through the `attribute_names` dictionary inside the
/// statement, once per overlap operand, so each axis keeps an operand its own
/// index serves. They bind as one `text[]` per operand rather than a
/// placeholder per name, so a large requested-attribute set cannot exhaust the
/// parameter ceiling.
#[must_use]
pub fn attribute_name_candidate<N: fmt::Display>(attribute_names: &[N]) -> SimpleExpr {
    let requested_keys = || {
        let mut dictionary = Query::select();
        dictionary
            .expr(SeaExpr::col((RequestedName, AttributeNames::AttributeNameKey)))
            .from_as(AttributeNames::Table, RequestedName)
            .and_where(is_any_of(
                SeaExpr::col((RequestedName, AttributeNames::Name)),
                attribute_names.iter().map(ToString::to_string),
            ));
        array_of_subquery(dictionary)
    };
    Condition::any()
        .add(ri_col(RegistrationInformation::PropertyNameKeys).array_overlaps(requested_keys()))
        .add(ri_col(RegistrationInformation::RelationshipNameKeys).array_overlaps(requested_keys()))
        .add(declares_no_name())
        .into()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_postgresql_seaquery::{
        collation::{PostgresCollation, collate},
        literal::int_literal,
    };
    use sea_query::{PostgresQueryBuilder, Query};

    /// The migration that declares the indexes the tests here must hit.
    const MIGRATION: &str = include_str!("../../../../migrations/tenant/0011_registration_information.sql");

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn render(expression: SimpleExpr) -> String {
        let sql = Query::select().expr(expression).to_string(PostgresQueryBuilder);
        sql.trim_start_matches("SELECT ").to_owned()
    }

    /// The comparable spelling of an expression: case, layout, quoting,
    /// parentheses and the `ri` qualifier are what the migration's formatter
    /// and sea-query spell differently while PostgreSQL reads them alike.
    fn comparable(expression: &str) -> String {
        expression
            .to_lowercase()
            .chars()
            .filter(|character| !character.is_whitespace() && !matches!(character, '"' | '(' | ')'))
            .collect::<String>()
            .replace("ri.", "")
    }

    /// The text between `create index <name> on registration_information` and
    /// its `;`, without the access method: the indexed expression, and its
    /// `where` predicate when the index is partial.
    fn index_definition(name: &str) -> String {
        let head = format!("create index {name} on registration_information");
        let start = MIGRATION.find(&head).map(|position| position + head.len()).expect("the index is declared");
        let end = MIGRATION[start..].find(';').map(|position| start + position).expect("the index ends");
        MIGRATION[start..end].trim().trim_start_matches("using gin").to_owned()
    }

    #[test]
    fn every_preselect_expression_is_the_index_expression() {
        for (index, expression) in [
            ("idx_registration_information_entity_types", entity_types_or_wildcard()),
            ("idx_registration_information_entity_id", entity_id_or_none()),
            (
                "idx_registration_information_property_name_keys",
                ri_col(RegistrationInformation::PropertyNameKeys),
            ),
            (
                "idx_registration_information_relationship_name_keys",
                ri_col(RegistrationInformation::RelationshipNameKeys),
            ),
        ] {
            pretty_assertions::assert_eq!(comparable(&index_definition(index)), comparable(&render(expression)), "{index}");
        }
        let pattern_index = index_definition("idx_registration_information_entity_id_pattern");
        let (indexed, predicate) = pattern_index.split_once("where").expect("the pattern index is partial");
        pretty_assertions::assert_eq!(comparable(indexed), "entity_id_pattern");
        assert!(
            comparable(&render(entity_id_candidate(&[uri("urn:ngsi-ld:Vehicle:1")]))).ends_with(&comparable(predicate)),
            "the pattern branch is the partial index's predicate"
        );
        let undeclared_index = index_definition("idx_registration_information_undeclared_names");
        let (indexed, predicate) = undeclared_index.split_once("where").expect("the undeclared-names index is partial");
        pretty_assertions::assert_eq!(comparable(indexed), "registration_id");
        pretty_assertions::assert_eq!(comparable(predicate), comparable(&render(declares_no_name().into())));
        let trigram_index = index_definition("idx_registration_information_entity_id_trgm");
        let (indexed, opclass) = trigram_index.trim().rsplit_once(' ').expect("the trigram index names its opclass");
        let tested = collate(ri_col(RegistrationInformation::EntityId), &PostgresCollation::database_default());
        pretty_assertions::assert_eq!(
            comparable(indexed),
            comparable(&render(tested)),
            "the column under the collation a pattern test reads"
        );
        pretty_assertions::assert_eq!(comparable(opclass), "gin_trgm_ops");
    }

    /// Clause 5.12: a row without an `id` matches any queried pattern, whether
    /// it constrains the identifier by no pattern or by one of its own; a row
    /// with an `id` matches when the pattern matches it.
    #[test]
    fn the_pattern_test_names_the_identifier_sentinel_beside_the_match_on_the_trigram_indexed_column() {
        let pattern = Pattern::parse("urn:ngsi-ld:Fleet.*").expect("valid pattern");
        let sql = render(entity_id_pattern_candidate(&pattern));
        pretty_assertions::assert_eq!(
            sql,
            "COALESCE(\"ri\".\"entity_id\", '') = ('') OR (((\"ri\".\"entity_id\") COLLATE \"default\") ~ 'urn:ngsi-ld:Fleet.*')"
        );
    }

    /// Clause 4.9: the store evaluates the portable restatement, so `(?i)`
    /// reaches PostgreSQL with every option applied.
    #[test]
    fn the_pattern_test_matches_the_portable_restatement_not_the_written_pattern() {
        let pattern = Pattern::parse("(?i)urn:ngsi-ld:fleet.*").expect("valid pattern");
        let sql = render(entity_id_pattern_candidate(&pattern));
        assert!(!sql.contains("(?i)"), "{sql}");
        assert!(sql.contains(pattern.portable().as_str()), "{sql}");
    }

    #[test]
    fn the_type_test_overlaps_the_requested_types_beside_the_wildcard_sentinel() {
        let (sql, values) = Query::select()
            .expr(int_literal(1))
            .and_where(entity_type_candidate(&[iri("https://example.com/Vehicle")]))
            .build(PostgresQueryBuilder);
        assert!(sql.contains("COALESCE(\"ri\".\"entity_types\", '{*}') && $1"), "{sql}");
        pretty_assertions::assert_eq!(values.0.len(), 1, "the type and the sentinel cross as one array: {sql}");
        let rendered = render(entity_type_candidate(&[iri("https://example.com/Vehicle")]));
        assert!(rendered.contains("ARRAY ['https://example.com/Vehicle','*']"), "{rendered}");
    }

    /// Clause 4.9 regular expressions are decided by the residual over the
    /// stored restatement: a pattern row is a candidate whatever it holds.
    #[test]
    fn the_identifier_test_names_the_identifiers_beside_the_sentinel_and_admits_pattern_rows_unevaluated() {
        let sql = render(entity_id_candidate(&[uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")]));
        assert!(
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','urn:ngsi-ld:Vehicle:2',''])"),
            "{sql}"
        );
        assert!(sql.contains("OR \"ri\".\"entity_id_pattern\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains(" ~ "), "the identifier test runs no regular expression: {sql}");
    }

    /// Clause 5.12: an item naming no property and no relationship matches any
    /// attribute list; an axis an item leaves out is `NULL`, which overlaps
    /// nothing.
    #[test]
    fn each_axis_overlaps_the_requested_keys_as_stored_beside_the_branch_for_an_item_declaring_no_name() {
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed")]));
        for axis in ["property_name_keys", "relationship_name_keys"] {
            assert!(sql.contains(&format!("\"ri\".\"{axis}\" && (ARRAY (SELECT")), "{axis}: {sql}");
        }
        assert!(
            sql.ends_with(" OR (\"ri\".\"property_name_keys\" IS NULL AND \"ri\".\"relationship_name_keys\" IS NULL)"),
            "{sql}"
        );
        assert!(
            !sql.contains("CASE"),
            "each branch is an indexed column or the partial index's predicate: {sql}"
        );
    }

    #[test]
    fn the_attribute_test_resolves_names_through_the_dictionary_once_per_overlap() {
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed"), iri("https://example.com/temp")]));
        assert!(sql.contains("FROM \"attribute_names\" AS \"an\" WHERE \"an\".\"name\" = ANY("), "{sql}");
        pretty_assertions::assert_eq!(sql.matches("'https://example.com/speed'").count(), 2, "{sql}");
        pretty_assertions::assert_eq!(sql.matches("'https://example.com/temp'").count(), 2, "{sql}");
        assert!(!sql.contains('$'), "every placeholder is bound: {sql}");
    }
}
