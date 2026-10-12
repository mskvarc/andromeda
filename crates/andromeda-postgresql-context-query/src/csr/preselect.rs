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
//! - **Attribute names.** Each axis reads `{0}` on a row whose item declares no
//!   name at all, `{}` on the axis an item leaves out while naming attributes on
//!   the other, and the declared keys otherwise. The requested names resolve to
//!   their `attribute_names` keys in the statement, and the keys with `0`
//!   appended overlap either axis. Dictionary keys start at 1, so the test is
//!   exact: a row matches when its item declares no name or declares one of the
//!   requested names on either axis (clause 5.12). A name the dictionary never
//!   saw resolves to no key and matches only rows declaring no name.

use crate::csr::information_row::ri_col;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::{csr::RegistrationInformation, name_dictionary::AttributeNames};
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::text_literal,
    pg_array::{any, array_append, array_of_subquery, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use iri_rs::IriBuf;
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Func, Query, SimpleExpr};

/// The type-axis sentinel: the row's item has no `entities` and admits every
/// type. It names no type.
pub const ANY_TYPE: &str = "*";

/// The identifier-axis sentinel: the row names no exact identifier.
const ANY_IDENTIFIER: &str = "";

/// The attribute-axis sentinel: the row's item declares no name on either axis.
const ANY_ATTRIBUTE_KEY: i32 = 0;

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

/// The keys of one attribute axis as its index reads them: `{0}` when the item
/// declares no name at all, `{}` when it names attributes only on the other
/// axis, the declared keys otherwise.
fn declared_name_keys(axis: RegistrationInformation) -> SimpleExpr {
    let declares_nothing = Condition::all()
        .add(ri_col(RegistrationInformation::PropertyNameKeys).is_null())
        .add(ri_col(RegistrationInformation::RelationshipNameKeys).is_null());
    CaseStatement::new()
        .case(declares_nothing, text_literal(&format!("{{{ANY_ATTRIBUTE_KEY}}}")))
        .finally(Func::coalesce([ri_col(axis), text_literal("{}")]))
        .into()
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
/// none, or carries a pattern, which the caller's residual evaluates.
#[must_use]
pub fn entity_id_candidate(entity_ids: &[Identifier]) -> SimpleExpr {
    let named = entity_ids.iter().map(Identifier::as_str).chain([ANY_IDENTIFIER]);
    entity_id_or_none()
        .is_in(named)
        .or(ri_col(RegistrationInformation::EntityIdPattern).is_not_null())
}

/// Whether the row's item declares one of `attribute_names`, or no name at all
/// (clause 5.12).
///
/// The names resolve through the `attribute_names` dictionary inside the
/// statement, once per overlap operand, so each axis keeps an operand its own
/// index serves. They bind as one `text[]` per operand rather than a
/// placeholder per name, so a large requested-attribute set cannot exhaust the
/// parameter ceiling.
#[must_use]
pub fn attribute_name_candidate(attribute_names: &[IriBuf]) -> SimpleExpr {
    let names: Vec<&str> = attribute_names.iter().map(IriBuf::as_str).collect();
    let requested_keys = || {
        let an = Alias::new("an");
        let mut dictionary = Query::select();
        dictionary
            .expr(SeaExpr::col((an.clone(), AttributeNames::AttributeNameKey)))
            .from_as(AttributeNames::Table, an.clone())
            .and_where(SeaExpr::col((an, AttributeNames::Name)).eq(any(text_array(&names))));
        array_append(cast(array_of_subquery(dictionary), PostgresType::IntegerArray), SeaExpr::val(ANY_ATTRIBUTE_KEY))
    };
    declared_name_keys(RegistrationInformation::PropertyNameKeys)
        .array_overlaps(requested_keys())
        .or(declared_name_keys(RegistrationInformation::RelationshipNameKeys).array_overlaps(requested_keys()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    /// The migration that declares the indexes the tests here must hit.
    const MIGRATION: &str = include_str!("../../../../migrations/tenant/0014_registration_information.sql");

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
                declared_name_keys(RegistrationInformation::PropertyNameKeys),
            ),
            (
                "idx_registration_information_relationship_name_keys",
                declared_name_keys(RegistrationInformation::RelationshipNameKeys),
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
    }

    #[test]
    fn the_type_test_overlaps_the_requested_types_beside_the_wildcard_sentinel() {
        let (sql, values) = Query::select()
            .expr(SeaExpr::cust("1"))
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
            sql.contains("COALESCE(\"ri\".\"entity_id\", '') IN ('urn:ngsi-ld:Vehicle:1', 'urn:ngsi-ld:Vehicle:2', '')"),
            "{sql}"
        );
        assert!(sql.contains("OR \"ri\".\"entity_id_pattern\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains(" ~ "), "the identifier test runs no regular expression: {sql}");
    }

    /// Clause 5.12: an item naming no property and no relationship matches any
    /// attribute list (`{0}` on both axes meets the appended `0`); an item
    /// naming attributes on one axis only reads `{}` on the other, which meets
    /// nothing.
    #[test]
    fn an_absent_axis_matches_nothing_when_the_other_names_attributes_and_everything_when_both_are_absent() {
        let sql = render(attribute_name_candidate(&[iri("https://example.com/speed")]));
        for axis in ["property_name_keys", "relationship_name_keys"] {
            assert!(
                sql.contains(&format!(
                    "(CASE WHEN (\"ri\".\"property_name_keys\" IS NULL AND \"ri\".\"relationship_name_keys\" IS NULL) THEN '{{0}}' ELSE COALESCE(\"ri\".\"{axis}\", '{{}}') END) && array_append(ARRAY((SELECT"
                )),
                "{axis}: {sql}"
            );
        }
        assert!(sql.contains("::integer[], 0)"), "the sentinel is appended to the resolved keys: {sql}");
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
