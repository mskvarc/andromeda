//! Whether some member of a structured value is a Query Term operand under
//! the data type its declaration gives it.
//!
//! JSONB containment compares JSON types, while clause 4.9 compares data
//! types: a JSON string declared as an XSD number is a number, one declared
//! as `xsd:boolean` a Boolean, one declared as a clause 4.6.3 time value that
//! time value, and every other JSON string a String. A member is declared by
//! the instance's `valueType` when it has one, which declares every primitive
//! member of its value (clause 4.5.2), and by its own `@type` otherwise. A
//! containment probe that matches a JSON string therefore only narrows the
//! candidates, and each member of a candidate is read the way one JSON value
//! is read ([`lift_comparison`]) to decide it.

use crate::{jsonb::lift_comparison, operator::compare};
use andromeda_ngsi_ld_query::filter::{predicate::CmpOp, scalar::Scalar};
use andromeda_ngsi_ld_vocabulary::keyword;
use andromeda_postgresql_schema::name_dictionary::EntityTypeNames;
use andromeda_postgresql_seaquery::{
    json_type::{JsonType, jsonb_type_is},
    jsonb::jsonb_array_elements,
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
};
use sea_query::{Alias, CaseStatement, Expr as SeaExpr, ExprTrait, Func, JoinType, Query, SimpleExpr};

/// The alias one member of the structured value is read under.
const MEMBER: &str = "dm";

/// The output column of `jsonb_array_elements`.
const MEMBER_VALUE: &str = "value";

/// The alias the dictionary row naming the instance's `valueType` is read
/// under.
const VALUE_TYPE: &str = "dmt";

/// Whether some member of the structured value in `column`, `value_type`
/// being the instance's `valueType` key, equals `scalar`.
///
/// A member is its own value when it is a scalar and its `@value` when it is
/// a value node. A value that is not an array holds no member: the array test
/// sits inside a `CASE` because the conjuncts of a `WHERE` run in whatever
/// order the planner picks, and unnesting an object raises an error rather
/// than yielding no rows.
#[must_use]
pub fn some_member_equals(column: &SimpleExpr, value_type: &SimpleExpr, scalar: &Scalar) -> SimpleExpr {
    // clone: sea-query owns every operand, and one column is read by several tests
    let array = CaseStatement::new().case(jsonb_type_is(column.clone(), JsonType::Array), column.clone());
    let member = || SimpleExpr::from(SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER_VALUE))));
    let stored = SimpleExpr::from(Func::coalesce([member().json_get(text_literal(keyword::VALUE)), member()]));
    let declared_by_value_type = SimpleExpr::from(SeaExpr::col((Alias::new(VALUE_TYPE), EntityTypeNames::Name)));
    let datatype = SimpleExpr::from(Func::coalesce([declared_by_value_type, member().json_get_text(text_literal(keyword::TYPE))]));
    let lifted = lift_comparison(scalar, stored, datatype);
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_array_elements(array.into()), Alias::new(MEMBER))
        .join_as(
            JoinType::LeftJoin,
            EntityTypeNames::Table,
            Alias::new(VALUE_TYPE),
            // clone: sea-query owns every operand, and one key is read by several tests
            SeaExpr::col((Alias::new(VALUE_TYPE), EntityTypeNames::EntityTypeNameKey)).eq(value_type.clone()),
        )
        .and_where(compare(lifted.stored, CmpOp::Equal, lifted.operand));
    SeaExpr::exists(scan)
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::declared_member::some_member_equals;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::filter::scalar::Scalar;
    use sea_query::{Alias, Expr as SeaExpr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn column() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_object"))
    }

    fn value_type() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_type_key"))
    }

    #[test]
    fn only_an_array_is_unnested() {
        let sql = render(some_member_equals(&column(), &value_type(), &Scalar::Bool(true)));
        assert!(
            sql.contains(r#"jsonb_array_elements((CASE WHEN (jsonb_typeof("value_object") = ('array')) THEN "value_object" END)) AS "dm""#),
            "{sql}"
        );
    }

    /// Clause 4.5.2: the `valueType` declares each member before its own
    /// `@type` does.
    #[test]
    fn a_member_is_declared_by_the_value_type_before_its_own_type() {
        let sql = render(some_member_equals(&column(), &value_type(), &Scalar::Number(JsonNumber::from(4))));
        assert!(
            sql.contains(r#"LEFT JOIN "entity_type_names" AS "dmt" ON "dmt"."entity_type_name_key" = "value_type_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#"COALESCE("dmt"."name", "dm"."value" ->> ('@type'))"#), "{sql}");
        assert!(sql.contains(r#"COALESCE("dm"."value" -> ('@value'), "dm"."value")"#), "{sql}");
    }

    #[test]
    fn a_number_member_reads_a_string_declared_as_an_xsd_number() {
        let sql = render(some_member_equals(&column(), &value_type(), &Scalar::Number(JsonNumber::from(4))));
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#integer'"), "{sql}");
        assert!(sql.contains("::numeric"), "{sql}");
    }

    #[test]
    fn a_string_member_leaves_out_a_string_declared_as_another_data_type() {
        let sql = render(some_member_equals(&column(), &value_type(), &Scalar::Str("4".to_owned())));
        assert!(sql.contains("NOT"), "{sql}");
        assert!(sql.contains("'http://www.w3.org/2001/XMLSchema#boolean'"), "{sql}");
    }
}
