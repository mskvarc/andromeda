//! Calls of the sub-attribute equality key functions every tenant defines.
//!
//! `sub_eq_keys(attribute_name_key, sub_attributes)` is the expression
//! `idx_ca_sub_eq_keys` indexes, one key per scalar value of a sub-attribute
//! tree, and `sub_eq_query_key(path, …)` the one key an operand has at a name
//! path. A query reaches the index only by naming the indexed expression
//! exactly, so both calls are rendered here, once.

use crate::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, Func, Iden, SimpleExpr};

/// The key function `idx_ca_sub_eq_keys` indexes.
#[derive(Iden, Clone, Copy)]
#[iden = "sub_eq_keys"]
struct SubEqKeys;

/// The function returning the one key an operand has at a name path.
#[derive(Iden, Clone, Copy)]
#[iden = "sub_eq_query_key"]
struct SubEqQueryKey;

/// `sub_eq_keys(attribute_name_key, sub_attributes)`.
#[must_use]
pub fn sub_eq_keys(attribute_name_key: SimpleExpr, sub_attributes: SimpleExpr) -> SimpleExpr {
    Func::cust(SubEqKeys).args([attribute_name_key, sub_attributes]).into()
}

/// `sub_eq_query_key(path, value)`: the key of a number, a boolean or an
/// instant, the overload chosen by the type `value` is cast to.
#[must_use]
pub fn sub_eq_query_key(path: SimpleExpr, value: SimpleExpr) -> SimpleExpr {
    Func::cust(SubEqQueryKey).args([path, value]).into()
}

/// `sub_eq_query_key(path, kind::smallint, text::text)`: the key of a text
/// held under the value kind `kind`, which salts it.
#[must_use]
pub fn sub_eq_query_text_key(path: SimpleExpr, kind: i16, text: String) -> SimpleExpr {
    Func::cust(SubEqQueryKey)
        .args([path, cast(Expr::val(kind), PostgresType::Smallint), cast(Expr::val(text), PostgresType::Text)])
        .into()
}

#[cfg(test)]
mod tests {
    use crate::sub_attribute_key::{sub_eq_keys, sub_eq_query_text_key};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_key_calls_name_the_tenant_functions() {
        let sql = Query::select()
            .expr(sub_eq_keys(Expr::col(Alias::new("k")), Expr::col(Alias::new("s"))))
            .expr(sub_eq_query_text_key(Expr::col(Alias::new("p")), 2, "x".to_owned()))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT sub_eq_keys("k", "s"), sub_eq_query_key("p", 2::smallint, 'x'::text)"#);
    }
}
