//! Composite values written from JSON and read back as JSON.
//!
//! A composite column is bound in one piece by the broker's own types. A
//! statement that writes a composite of a shape no broker type binds (the shape
//! an earlier release stored, written so its upgrade can be rehearsed) fills it
//! from a JSON document with `jsonb_populate_record`, field by field and
//! recursing into composite and array fields, and reads it back with `to_jsonb`.

use crate::{
    cast::{cast, typed_null},
    pg_type::PostgresType,
};
use sea_query::{Func, Iden, SimpleExpr};

/// The `jsonb_populate_record` function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_populate_record"]
struct JsonbPopulateRecord;

/// The `to_jsonb` function.
#[derive(Iden, Clone, Copy)]
#[iden = "to_jsonb"]
struct ToJsonb;

/// A value of the composite type `composite` whose fields `document` names,
/// the fields it omits left NULL.
///
/// `composite` names a composite type the migrations declare; PostgreSQL
/// refuses any other type. `document` is any expression of JSON text or
/// `jsonb`, and a field that is itself composite or an array takes a JSON
/// object or array of the matching shape.
#[must_use]
pub fn composite_from_json(composite: PostgresType, document: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbPopulateRecord)
        .args([typed_null(composite), cast(document, PostgresType::Jsonb)])
        .into()
}

/// The JSON document of `value`, a composite as an object keyed by field
/// name and an array as an array of such objects.
#[must_use]
pub fn to_jsonb(value: SimpleExpr) -> SimpleExpr {
    Func::cust(ToJsonb).args([value]).into()
}

#[cfg(test)]
mod tests {
    use crate::{
        json_record::{composite_from_json, to_jsonb},
        pg_type::PostgresType,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_composite_is_populated_from_a_document_cast_to_jsonb() {
        let sql = Query::select()
            .expr(composite_from_json(PostgresType::SubAttribute, Expr::val("{}")))
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT jsonb_populate_record(NULL::sub_attribute, '{}'::jsonb)");
    }

    #[test]
    fn a_value_is_read_back_through_to_jsonb() {
        let sql = Query::select()
            .expr(to_jsonb(Expr::col(Alias::new("sub_attributes"))))
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT to_jsonb("sub_attributes")"#);
    }
}
