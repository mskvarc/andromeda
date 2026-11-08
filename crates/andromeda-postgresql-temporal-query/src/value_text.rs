//! One attribute history row's value as text, whatever column holds it.
//!
//! A row holds its value in the one typed column its value kind names, so the
//! value as text is the coalesce of every value column, each cast to text.
//! `distinctCount` counts distinct values of it and an `aggrGroupBy` value key
//! groups on it, so both fold the same columns, which are read off
//! [`ValueKind::history_column`] rather than listed again.

use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, Func, IntoIden, SimpleExpr};

/// The value of the attribute history row under `row`, as text; SQL `NULL` for
/// a tombstone.
#[must_use]
pub(crate) fn value_text(row: impl IntoIden) -> SimpleExpr {
    let row = row.into_iden();
    let mut columns: Vec<AttributeHistory> = Vec::new();
    for column in ValueKind::ALL.iter().filter_map(|kind| kind.history_column()) {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    Func::coalesce(columns.into_iter().map(|column| cast(Expr::col((row.clone(), column)), PostgresType::Text))).into()
}

#[cfg(test)]
mod tests {
    use crate::value_text::value_text;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn every_value_column_is_folded_once() {
        let sql = Query::select().expr(value_text(Alias::new("ah"))).to_owned().to_string(PostgresQueryBuilder);
        for column in [
            "value_number",
            "value_string",
            "value_boolean",
            "value_geometry",
            "value_object",
            "value_date_time",
            "value_uri",
        ] {
            assert_eq!(sql.matches(&format!(r#""ah"."{column}""#)).count(), 1, "{column}: {sql}");
        }
    }
}
