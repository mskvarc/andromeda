//! Whether some member of a structured value is a Query Term operand under
//! the data type its declaration gives it.
//!
//! JSONB containment compares JSON types, while clause 4.9 compares data
//! types: a JSON string declared as an XSD number is a number, one declared
//! as `xsd:boolean` a Boolean, one declared as a clause 4.6.3 time value that
//! time value, and every other JSON string a String. A member is declared by
//! the `valueType` of a Property or a `ListProperty` when it has one, which
//! declares every primitive member of its value (clause 4.5.2), and by its own
//! `@type` otherwise. A
//! containment probe that matches a JSON string therefore only narrows the
//! candidates, and each member of a candidate is read the way one JSON value
//! is read ([`lift_comparison`]) to decide it.

use crate::{jsonb::lift_comparison, operator::compare};
use andromeda_ngsi_ld_query::filter::reading::{comparator::Comparator, operand::Operand};
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

/// Whether some member of the structured value in `column` equals `operand`,
/// `value_type` being the `valueType` key that declares the members, NULL
/// when none does.
///
/// A member is its own value when it is a scalar and its `@value` when it is
/// a value node. A value that is not an array holds no member: the array test
/// sits inside a `CASE` because the conjuncts of a `WHERE` run in whatever
/// order the planner picks, and unnesting an object raises an error rather
/// than yielding no rows.
#[must_use]
pub fn some_member_equals(column: &SimpleExpr, value_type: &SimpleExpr, operand: &Operand) -> SimpleExpr {
    // clone: sea-query owns every operand, and one column is read by several tests
    let array = CaseStatement::new().case(jsonb_type_is(column.clone(), JsonType::Array), column.clone());
    let member = || SimpleExpr::from(SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER_VALUE))));
    let stored = SimpleExpr::from(Func::coalesce([member().json_get(text_literal(keyword::VALUE)), member()]));
    let declared_by_value_type = SimpleExpr::from(SeaExpr::col((Alias::new(VALUE_TYPE), EntityTypeNames::Name)));
    let datatype = SimpleExpr::from(Func::coalesce([declared_by_value_type, member().json_get_text(text_literal(keyword::TYPE))]));
    let lifted = lift_comparison(operand, stored, datatype);
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
        .and_where(compare(lifted.stored, Comparator::Equal, lifted.operand));
    SeaExpr::exists(scan)
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::declared_member::some_member_equals;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::filter::reading::operand::Operand;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr as SeaExpr, Query, SelectStatement, SimpleExpr};

    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    fn column() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_object"))
    }

    fn value_type() -> SimpleExpr {
        SeaExpr::col(Alias::new("value_type_key"))
    }

    #[test]
    fn only_an_array_is_unnested() {
        let statement = render(some_member_equals(&column(), &value_type(), &Operand::Boolean(true)));
        assert_statement_snapshot!("declared-member-only-an-array-is-unnested", statement);
    }

    /// Clause 4.5.2: the `valueType` declares each member before its own
    /// `@type` does.
    #[test]
    fn a_member_is_declared_by_the_value_type_before_its_own_type() {
        let statement = render(some_member_equals(&column(), &value_type(), &Operand::Number(JsonNumber::from(4))));
        assert_statement_snapshot!("declared-member-a-member-is-declared-by-the-value-type-before-its-own-type", statement);
    }

    #[test]
    fn a_number_member_reads_a_string_declared_as_an_xsd_number() {
        let statement = render(some_member_equals(&column(), &value_type(), &Operand::Number(JsonNumber::from(4))));
        assert_statement_snapshot!("declared-member-a-number-member-reads-a-string-declared-as-an-xsd-number", statement);
    }

    #[test]
    fn a_string_member_leaves_out_a_string_declared_as_another_data_type() {
        let statement = render(some_member_equals(&column(), &value_type(), &Operand::Text("4".to_owned())));
        assert_statement_snapshot!("declared-member-a-string-member-leaves-out-a-string-declared-as-another-data-type", statement);
    }
}
