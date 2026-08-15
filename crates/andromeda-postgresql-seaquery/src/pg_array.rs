//! PostgreSQL array constructs sea-query has no native form for.

use sea_query::{Expr, Func, Iden, PostgresQueryBuilder, SelectStatement, SimpleExpr};

/// `ARRAY(subquery)` where the subquery carries bound values, collecting a
/// single-column result set into one Postgres array.
///
/// Unlike [`array_subquery`], the subquery is embedded as a sub-statement rather
/// than rendered to text, so any request value inside it — a name to resolve
/// through a dictionary, a type to overlap — binds and numbers with the enclosing
/// statement rather than being inlined.
pub fn array_of_subquery(subquery: SelectStatement) -> SimpleExpr {
    Expr::cust_with_exprs("ARRAY($1)", [SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))])
}

/// The `ANY` array quantifier.
#[derive(Iden, Clone, Copy)]
#[iden = "ANY"]
struct AnyQuantifier;

/// `ARRAY(subquery)` - the array-valued subquery constructor, which collects a
/// single-column result set into one Postgres array.
///
/// Distinct from `ARRAY[…]`: the operand is a `SELECT`, not an element list.
/// sea-query has no form for it, so the subquery is rendered and wrapped here.
/// The subquery's text is embedded rather than parameterised, so every operand
/// in it must be a column or function call — a value-free subquery, which is the
/// case for `ARRAY(SELECT jsonb_array_elements_text(col))`: no request value
/// crosses this boundary, only columns of the enclosing statement.
pub fn array_subquery(subquery: SelectStatement) -> SimpleExpr {
    Expr::cust(format!("ARRAY({})", subquery.to_string(PostgresQueryBuilder)))
}

/// `ANY(array)` - the right operand of `column = ANY(array)`.
///
/// Membership is tested against one bound array rather than expanding to a
/// placeholder per element, so a large batch cannot exhaust the wire protocol's
/// parameter space. The array binds as a single value.
pub fn any(array: SimpleExpr) -> SimpleExpr {
    Func::cust(AnyQuantifier).args([array]).into()
}

/// A bound `text[]` value from a batch of borrowed strings.
///
/// The array value owns its elements, so each string is copied: a bound
/// parameter cannot borrow from the caller's slice across the round trip.
pub fn text_array(values: &[&str]) -> SimpleExpr {
    Expr::val(values.iter().map(|value| (*value).to_owned()).collect::<Vec<String>>()).into()
}

#[cfg(test)]
mod tests {
    use crate::pg_array::{any, array_subquery};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render_expr(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_array_subquery_wraps_a_select_in_the_array_constructor() {
        let subquery = Query::select().expr(Expr::col(Alias::new("value"))).to_owned();
        let sql = render_expr(array_subquery(subquery));
        assert!(sql.contains(r#"ARRAY(SELECT "value")"#), "{sql}");
    }

    #[test]
    fn equality_against_any_binds_the_array_once() {
        let (sql, values) = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("attribute_names"))
            .and_where(Expr::col(Alias::new("name")).eq(any(Expr::val(vec!["a".to_string(), "b".to_string()]).into())))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("= ANY("), "{sql}");
        assert_eq!(values.0.len(), 1, "the whole array is one bound value: {sql}");
    }
}
