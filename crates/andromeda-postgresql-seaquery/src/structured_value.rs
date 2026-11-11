//! The structured value of a `sub_attribute` element, read through the
//! `sub_attribute_value_object` function the tenant migrations define: the
//! element's `value_object`, or for a data-integrity seal (ETSI GS CIM 009
//! v1.9.1 clause 4.5.2.2) the expanded proof its `value_proof` holds, as the
//! model writes a structured value. A `q` filter and an ordering read an
//! element's structured value through it, so a seal's proof is compared as
//! any structured value is.

use sea_query::{Func, Iden, SimpleExpr};

/// `sub_attribute_value_object`.
#[derive(Iden, Clone, Copy)]
#[iden = "sub_attribute_value_object"]
struct SubAttributeValueObject;

/// The structured value of the element whose fields `fields` are, in the
/// function's parameter order: `value_kind`, `value_object` and
/// `value_proof`.
#[must_use]
pub fn sub_attribute_value_object(fields: [SimpleExpr; 3]) -> SimpleExpr {
    Func::cust(SubAttributeValueObject).args(fields).into()
}

#[cfg(test)]
mod tests {
    use crate::structured_value::sub_attribute_value_object;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_function_takes_the_fields_in_order() {
        let fields = ["a", "b", "c"].map(|field| Expr::col(Alias::new(field)));
        let sql = Query::select().expr(sub_attribute_value_object(fields)).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"sub_attribute_value_object("a", "b", "c")"#), "{sql}");
    }
}
