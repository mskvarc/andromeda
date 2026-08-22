//! The clause 5.12 attribute specification, rendered against one `RegistrationInfo`.
//!
//! Attribute names match an information item when no names were asked for,
//! when the item declares no property and no relationship name (only entities
//! were registered, so the source may hold any attribute), or when at least
//! one asked-for name is declared on either axis.

use crate::csr::unnesting::{info, lacks_key};
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_array_elements_text,
    literal::{int_literal, text_literal},
    pg_array::{any, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use iri_rs::IriBuf;
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Func, Query};

/// Whether the current information item's declared names meet `attribute_names`
/// (clause 5.12).
///
/// An item that names nothing on either axis registered entities only and
/// matches every attribute list; otherwise one of the names must be declared as
/// a property or as a relationship. A missing array is coalesced to an empty
/// one so an item naming nothing on one axis is simply no match on that axis.
pub(crate) fn attribute_names_declared(attribute_names: &[IriBuf]) -> Condition {
    let names: Vec<&str> = attribute_names.iter().map(IriBuf::as_str).collect();
    let member_of = |member: &'static str, alias: &'static str| {
        let coalesced = Func::coalesce([info().json_get(text_literal(member)), cast(text_literal("[]"), PostgresType::Jsonb)]);
        let mut scan = Query::select();
        scan.expr(int_literal(1))
            .from_function(jsonb_array_elements_text(coalesced.into()), Alias::new(alias))
            // One bound `text[]` rather than a placeholder per name, so a large
            // requested-attribute set cannot exhaust the parameter ceiling.
            .and_where(SeaExpr::expr(SeaExpr::col(Alias::new(alias))).eq(any(text_array(&names))));
        SeaExpr::exists(scan)
    };
    Condition::any()
        .add(
            Condition::all()
                .add(lacks_key(info(), "propertyNames"))
                .add(lacks_key(info(), "relationshipNames")),
        )
        .add(member_of("propertyNames", "pn"))
        .add(member_of("relationshipNames", "rn"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn render(names: &[IriBuf]) -> String {
        Query::select()
            .expr(int_literal(1))
            .from(Alias::new("context_source_registrations"))
            .cond_where(attribute_names_declared(names))
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_entity_only_item_matches_and_a_declared_name_on_either_axis_matches() {
        let sql = render(&[iri("https://example.com/speed"), iri("https://example.com/temp")]);
        assert!(
            sql.contains("(NOT jsonb_exists(\"info\".\"value\", 'propertyNames')) AND (NOT jsonb_exists(\"info\".\"value\", 'relationshipNames'))"),
            "{sql}"
        );
        assert!(sql.contains("'propertyNames'"), "{sql}");
        assert!(sql.contains("'relationshipNames'"), "{sql}");
        assert!(sql.contains("= ANY(ARRAY ['https://example.com/speed','https://example.com/temp'])"), "{sql}");
    }

    #[test]
    fn an_absent_axis_is_coalesced_to_an_empty_array_rather_than_null() {
        let sql = render(&[iri("https://example.com/speed")]);
        assert!(sql.contains("COALESCE(\"info\".\"value\" -> ('propertyNames'), '[]'::jsonb)"), "{sql}");
    }
}
