//! Ordered aggregate calls sea-query has no native form for.
//!
//! An aggregate whose result depends on the order its inputs are folded in
//! (`array_agg(x ORDER BY y)`, `string_agg(x, d ORDER BY y)`) carries an
//! `ORDER BY` clause inside the function call. sea-query models a function call
//! as a bare argument list with no in-call ordering, so the ordered form is
//! spelled here, in the one crate sanctioned to write a hand-formed fragment,
//! with both the aggregated value and the ordering key passed as bound
//! expressions rather than interpolated text.

use sea_query::{Expr, Func, Iden, SimpleExpr};

/// The `STDDEV(value)` sample-standard-deviation aggregate.
#[derive(Iden, Clone, Copy)]
#[iden = "STDDEV"]
struct StdDev;

/// The `BOOL_OR(predicate)` aggregate, true when the predicate held on any row.
#[derive(Iden, Clone, Copy)]
#[iden = "BOOL_OR"]
struct BoolOr;

/// The `LEAD(value)` window function, the value of the next row of the frame.
#[derive(Iden, Clone, Copy)]
#[iden = "LEAD"]
struct Lead;

/// The `LAG(value)` window function, the value of the previous row of the
/// frame.
#[derive(Iden, Clone, Copy)]
#[iden = "LAG"]
struct Lag;

/// The `ROW_NUMBER()` window function, the position of a row in its frame.
#[derive(Iden, Clone, Copy)]
#[iden = "ROW_NUMBER"]
struct RowNumber;

/// `STDDEV(value)` - the sample standard deviation of a group's values.
#[must_use]
pub fn stddev(value: SimpleExpr) -> SimpleExpr {
    Func::cust(StdDev).args([value]).into()
}

/// `BOOL_OR(predicate)` - whether `predicate` held on at least one row of the
/// group.
///
/// `MIN`/`MAX` skip nulls, so a group spanning both annotated and unannotated
/// rows needs this to report that some row carried no value at all.
#[must_use]
pub fn bool_or(predicate: SimpleExpr) -> SimpleExpr {
    Func::cust(BoolOr).args([predicate]).into()
}

/// `LEAD(value)` - `value` on the row after this one in its window frame, NULL
/// on the last row. The `OVER (…)` frame is supplied through
/// [`sea_query::SelectStatement::expr_window_as`].
#[must_use]
pub fn lead(value: SimpleExpr) -> SimpleExpr {
    Func::cust(Lead).args([value]).into()
}

/// `LAG(value)` - `value` on the row before this one in its window frame, NULL
/// on the first row. The `OVER (…)` frame is supplied through
/// [`sea_query::SelectStatement::expr_window_as`].
#[must_use]
pub fn lag(value: SimpleExpr) -> SimpleExpr {
    Func::cust(Lag).args([value]).into()
}

/// `ROW_NUMBER()` - the position of the row in its window frame, counted from
/// one. The `OVER (…)` frame is supplied through
/// [`sea_query::SelectStatement::expr_window_as`].
#[must_use]
pub fn row_number() -> SimpleExpr {
    Func::cust(RowNumber).into()
}

/// `array_agg(value ORDER BY order_by)` - collect `value` into an array in the
/// order `order_by` sorts the rows.
///
/// The ordering is part of the aggregate, not of the enclosing query, so a
/// caller that needs the array to preserve a per-row position folds that
/// position in here rather than ordering the surrounding statement.
#[must_use]
pub fn array_agg_ordered(value: SimpleExpr, order_by: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("array_agg($1 ORDER BY $2)", [value, order_by])
}

/// `array_agg(DISTINCT value)` - collect the distinct values into an array.
///
/// `DISTINCT` is part of the aggregate, which sea-query's bare argument list
/// cannot carry, so the call is spelled here.
#[must_use]
pub fn array_agg_distinct(value: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("array_agg(DISTINCT $1)", [value])
}

/// `array_agg(DISTINCT value) FILTER (WHERE predicate)` - collect the distinct
/// values of the rows the predicate keeps.
///
/// The `FILTER` restricts which rows feed the aggregate, which is how an absent
/// value stays out of the array rather than entering it as a NULL. Both the
/// value and the predicate are bound expressions.
#[must_use]
pub fn array_agg_distinct_filter(value: SimpleExpr, predicate: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("array_agg(DISTINCT $1) FILTER (WHERE $2)", [value, predicate])
}

/// `COUNT(DISTINCT value)` - the number of distinct non-NULL values.
#[must_use]
pub fn count_distinct(value: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("COUNT(DISTINCT $1)", [value])
}

/// `COUNT(*)` - the number of rows in the group, NULLs included.
#[must_use]
pub fn count_all() -> SimpleExpr {
    Expr::cust("COUNT(*)")
}

/// `COUNT(*) FILTER (WHERE predicate)` - the number of rows of the group the
/// predicate holds on.
#[must_use]
pub fn count_filtered(predicate: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("COUNT(*) FILTER (WHERE $1)", [predicate])
}

/// `COUNT(*) OVER ()` - the total number of rows the query returns, evaluated
/// once over the whole result and repeated on every row.
///
/// A paginated query piggybacks its total on every row this way, so the count
/// and the page come back in one round-trip. sea-query models a window frame as
/// a `WindowStatement`, but the empty `OVER ()` an unbounded count needs carries
/// no partition or ordering, so it is spelled here.
#[must_use]
pub fn count_over_all() -> SimpleExpr {
    Expr::cust("COUNT(*) OVER ()")
}

#[cfg(test)]
mod tests {
    use crate::aggregate::array_agg_ordered;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn the_aggregate_orders_its_inputs_inside_the_call() {
        let sql = Query::select()
            .expr(array_agg_ordered(Expr::col(Alias::new("name")), Expr::col(Alias::new("position"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"array_agg("name" ORDER BY "position")"#), "{sql}");
    }

    #[test]
    fn the_distinct_aggregates_carry_their_distinct_and_filter() {
        use crate::aggregate::{array_agg_distinct, array_agg_distinct_filter, count_all, count_distinct};
        let render = |expr| Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(render(array_agg_distinct(Expr::col(Alias::new("t")))).contains(r#"array_agg(DISTINCT "t")"#));
        assert!(
            render(array_agg_distinct_filter(
                Expr::col(Alias::new("at")),
                Expr::col(Alias::new("at")).is_not_null()
            ))
            .contains(r#"array_agg(DISTINCT "at") FILTER (WHERE "at" IS NOT NULL)"#)
        );
        assert!(render(count_distinct(Expr::col(Alias::new("id")))).contains(r#"COUNT(DISTINCT "id")"#));
        assert!(render(count_all()).contains("COUNT(*)"));
    }

    #[test]
    fn a_filtered_count_restricts_the_rows_it_counts() {
        use crate::aggregate::count_filtered;
        let sql = Query::select()
            .expr(count_filtered(Expr::col(Alias::new("ok"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT COUNT(*) FILTER (WHERE "ok")"#);
    }

    #[test]
    fn the_group_aggregates_render_their_own_function_names() {
        use crate::aggregate::{bool_or, lag, lead, row_number, stddev};
        let render = |expr| Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(render(stddev(Expr::col(Alias::new("v")))).contains(r#"STDDEV("v")"#));
        assert!(render(bool_or(Expr::col(Alias::new("u")).is_null())).contains(r#"BOOL_OR("u" IS NULL)"#));
        assert!(render(lead(Expr::col(Alias::new("s")))).contains(r#"LEAD("s")"#));
        assert!(render(lag(Expr::col(Alias::new("s")))).contains(r#"LAG("s")"#));
        assert!(render(row_number()).contains("ROW_NUMBER()"));
    }
}
