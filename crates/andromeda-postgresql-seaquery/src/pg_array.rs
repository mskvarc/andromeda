//! PostgreSQL array constructs sea-query has no native form for.

use sea_query::{Expr, Func, Iden, PostgresQueryBuilder, QueryStatementBuilder, SelectStatement, SimpleExpr};

/// `ARRAY(subquery)` where the subquery carries bound values, collecting a
/// single-column result set into one Postgres array.
///
/// Unlike [`array_subquery`], the subquery is embedded as a sub-statement rather
/// than rendered to text, so any request value inside it — a name to resolve
/// through a dictionary, a type to overlap — binds and numbers with the enclosing
/// statement rather than being inlined.
#[must_use]
pub fn array_of_subquery(subquery: SelectStatement) -> SimpleExpr {
    Expr::cust_with_exprs("ARRAY($1)", [SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))])
}

/// The `ANY` array quantifier.
#[derive(Iden, Clone, Copy)]
#[iden = "ANY"]
struct AnyQuantifier;

/// The `array_position` function.
#[derive(Iden, Clone, Copy)]
#[iden = "array_position"]
struct ArrayPosition;

/// `array_position(array, element)` - the one-based position of the first
/// occurrence of `element` in `array`, or NULL when it is absent.
///
/// The ordering key that reads a dictionary-keyed array back in its stored
/// order: a subquery over the dictionary is sorted by each key's position in
/// the row's own array, so a rendered type list comes back in the order the
/// entity declared it.
#[must_use]
pub fn array_position(array: SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    Func::cust(ArrayPosition).args([array, element]).into()
}

/// The `array_append` function.
#[derive(Iden, Clone, Copy)]
#[iden = "array_append"]
struct ArrayAppend;

/// `array_append(array, element)` - `array` with `element` added at its end.
///
/// The way a routing predicate names a sentinel beside the values it resolved:
/// an overlap against `array_append(ARRAY(<resolved keys>), 0)` admits both the
/// rows declaring one of the keys and the rows whose stored set is the
/// wildcard `{0}`, in one operator a GIN index serves.
#[must_use]
pub fn array_append(array: SimpleExpr, element: SimpleExpr) -> SimpleExpr {
    Func::cust(ArrayAppend).args([array, element]).into()
}

/// `ARRAY(subquery)` - the array-valued subquery constructor, which collects a
/// single-column result set into one Postgres array.
///
/// Distinct from `ARRAY[…]`: the operand is a `SELECT`, not an element list.
/// sea-query has no form for it, so the subquery is rendered and wrapped here.
/// The subquery's text is embedded rather than parameterised, so every operand
/// in it must be a column or function call — a value-free subquery, which is the
/// case for `ARRAY(SELECT jsonb_array_elements_text(col))`: no request value
/// crosses this boundary, only columns of the enclosing statement.
#[must_use]
pub fn array_subquery(subquery: &SelectStatement) -> SimpleExpr {
    Expr::cust(format!("ARRAY({})", subquery.to_string(PostgresQueryBuilder)))
}

/// `ANY(array)` - the right operand of `column = ANY(array)`.
///
/// Membership is tested against one bound array rather than expanding to a
/// placeholder per element, so a large batch cannot exhaust the wire protocol's
/// parameter space. The array binds as a single value.
#[must_use]
pub fn any(array: SimpleExpr) -> SimpleExpr {
    Func::cust(AnyQuantifier).args([array]).into()
}

/// A bound `text[]` value from a batch of borrowed strings.
///
/// The array value owns its elements, so each string is copied: a bound
/// parameter cannot borrow from the caller's slice across the round trip.
#[must_use]
pub fn text_array(values: &[&str]) -> SimpleExpr {
    Expr::val(values.iter().map(|value| (*value).to_owned()).collect::<Vec<String>>())
}

#[cfg(test)]
mod tests {
    use crate::pg_array::{any, array_append, array_position, array_subquery};
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};

    fn render_expr(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn array_position_renders_the_two_argument_function() {
        let sql = render_expr(array_position(Expr::col(Alias::new("type_ids")), Expr::col(Alias::new("id"))));
        assert_eq!(sql, "SELECT array_position(\"type_ids\", \"id\")");
    }

    #[test]
    fn array_append_renders_the_two_argument_function() {
        let sql = render_expr(array_append(Expr::col(Alias::new("keys")), Expr::val(0)));
        assert_eq!(sql, "SELECT array_append(\"keys\", 0)");
    }

    #[test]
    fn an_array_subquery_wraps_a_select_in_the_array_constructor() {
        let subquery = Query::select().expr(Expr::col(Alias::new("value"))).to_owned();
        let sql = render_expr(array_subquery(&subquery));
        assert!(sql.contains(r#"ARRAY(SELECT "value")"#), "{sql}");
    }

    #[test]
    fn equality_against_any_binds_the_array_once() {
        let (sql, values) = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("attribute_names"))
            .and_where(Expr::col(Alias::new("name")).eq(any(Expr::val(vec!["a".to_string(), "b".to_string()]))))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("= ANY("), "{sql}");
        assert_eq!(values.0.len(), 1, "the whole array is one bound value: {sql}");
    }
}
