//! The statements every seed and every read is built from, and the typed
//! values they bind.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_seaquery::{
    cast::cast,
    json_record::{composite_from_json, to_jsonb},
    pg_array::array_literal,
    pg_type::PostgresType,
    statement::Statement,
};
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Order, Query, SelectStatement, SimpleExpr};
use serde_json::Value;
use sqlx::{FromRow, PgPool, postgres::PgRow};

/// A column or table name.
pub(crate) fn name(identifier: &str) -> Alias {
    Alias::new(identifier)
}

/// An instant written as RFC 3339 text.
pub(crate) fn instant(text: &str) -> SimpleExpr {
    cast(Expr::val(text), PostgresType::Timestamptz)
}

/// A UUID written as text.
pub(crate) fn uuid(text: &str) -> SimpleExpr {
    cast(Expr::val(text), PostgresType::Uuid)
}

/// A `numeric` written as text.
pub(crate) fn numeric(text: &str) -> SimpleExpr {
    cast(Expr::val(text), PostgresType::Numeric)
}

/// A `smallint`.
pub(crate) fn smallint(value: i16) -> SimpleExpr {
    Expr::val(value)
}

/// A `jsonb` document.
pub(crate) fn document(value: &Value) -> SimpleExpr {
    cast(Expr::val(value.to_string()), PostgresType::Jsonb)
}

/// A `json` document, kept as written.
pub(crate) fn json_document(value: &Value) -> SimpleExpr {
    cast(Expr::val(value.to_string()), PostgresType::Json)
}

/// An `integer[]`.
pub(crate) fn integers(values: &[i32]) -> SimpleExpr {
    cast(array_literal(values.iter().map(|value| Expr::val(*value))), PostgresType::IntegerArray)
}

/// A `smallint[]`.
pub(crate) fn smallints(values: &[i16]) -> SimpleExpr {
    cast(array_literal(values.iter().map(|value| Expr::val(*value))), PostgresType::SmallintArray)
}

/// A `text[]`, empty when no value is given.
pub(crate) fn texts(values: &[&str]) -> SimpleExpr {
    cast(array_literal(values.iter().map(|value| Expr::val(*value))), PostgresType::TextArray)
}

/// An array of composites of `composite`, one per document.
pub(crate) fn composites(composite: PostgresType, documents: &[Value]) -> SimpleExpr {
    array_literal(documents.iter().map(|value| composite_from_json(composite, Expr::val(value.to_string()))))
}

/// One composite of `composite` written from `value`.
pub(crate) fn composite(composite: PostgresType, value: &Value) -> SimpleExpr {
    composite_from_json(composite, Expr::val(value.to_string()))
}

/// Inserts one row, given as column and value pairs.
pub(crate) async fn insert(pool: &PgPool, table: &str, row: Vec<(&str, SimpleExpr)>) -> Result<(), UpgradeTestError> {
    let (columns, values): (Vec<Alias>, Vec<SimpleExpr>) = row.into_iter().map(|(column, value)| (name(column), value)).unzip();
    let statement = Query::insert().into_table(name(table)).columns(columns).values(values)?.to_owned();
    statement.to_query().execute(pool).await?;
    Ok(())
}

/// Inserts one row and reads back the generated `key` column.
pub(crate) async fn insert_returning<Key>(pool: &PgPool, table: &str, key: &str, row: Vec<(&str, SimpleExpr)>) -> Result<Key, UpgradeTestError>
where
    Key: Send + Unpin,
    (Key,): for<'row> FromRow<'row, PgRow>,
{
    let (columns, values): (Vec<Alias>, Vec<SimpleExpr>) = row.into_iter().map(|(column, value)| (name(column), value)).unzip();
    let statement = Query::insert()
        .into_table(name(table))
        .columns(columns)
        .values(values)?
        .returning_col(name(key))
        .to_owned();
    Ok(statement.to_query_scalar::<Key>().fetch_one(pool).await?)
}

/// `to_jsonb` of one column of `table`, for the rows `filter` keeps, in
/// `order_by` order. A NULL column reads as `None`.
pub(crate) async fn read_documents(
    pool: &PgPool,
    table: &str,
    column: &str,
    order_by: &str,
    filter: Option<(&str, i64)>,
) -> Result<Vec<Option<Value>>, UpgradeTestError> {
    let mut select = Query::select();
    select
        .expr(to_jsonb(Expr::col(name(column))))
        .from(name(table))
        .order_by(name(order_by), Order::Asc);
    if let Some((filter_column, filter_value)) = filter {
        select.and_where(Expr::col(name(filter_column)).eq(filter_value));
    }
    Ok(select.to_query_scalar::<Option<Value>>().fetch_all(pool).await?)
}

/// One column of `table`, a value per row, in the column's own order.
pub(crate) async fn read_column<Cell>(pool: &PgPool, table: &str, column: &str) -> Result<Vec<Cell>, UpgradeTestError>
where
    Cell: Send + Unpin,
    (Cell,): for<'row> FromRow<'row, PgRow>,
{
    let select: SelectStatement = Query::select()
        .column(name(column))
        .from(name(table))
        .order_by(name(column), Order::Asc)
        .to_owned();
    Ok(select.to_query_scalar::<Cell>().fetch_all(pool).await?)
}

/// How many rows `table` holds.
pub(crate) async fn count_rows(pool: &PgPool, table: &str) -> Result<i64, UpgradeTestError> {
    let select = Query::select().expr(Func::count(Expr::col(Asterisk))).from(name(table)).to_owned();
    Ok(select.to_query_scalar::<i64>().fetch_one(pool).await?)
}
