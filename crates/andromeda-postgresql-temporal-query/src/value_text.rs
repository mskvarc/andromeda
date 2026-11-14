//! One attribute history row's value as text, whatever field holds it.
//!
//! A row holds its value in the one field its value kind is placed in, so the
//! value as text is the coalesce of every value field, each cast to text.
//! `distinctCount` counts distinct values of it and an `aggrGroupBy` value key
//! groups on it, so both fold the same fields, which are read off
//! [`ValueField`] rather than listed again.

use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use andromeda_postgresql_stored_values::value_placement::ValueField;
use sea_query::{Expr, Func, IntoIden, SimpleExpr};
use strum::IntoEnumIterator;

/// The value of the attribute history row under `row`, as text; SQL `NULL` for
/// a tombstone.
#[must_use]
pub(crate) fn value_text(row: impl IntoIden) -> SimpleExpr {
    let row = row.into_iden();
    Func::coalesce(ValueField::iter().map(|field| cast(Expr::col((row.clone(), field.in_history())), PostgresType::Text))).into()
}

#[cfg(test)]
mod tests {
    use crate::value_text::value_text;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Query};

    #[test]
    fn every_value_column_is_folded_once() {
        let statement = Query::select().expr(value_text(Alias::new("ah"))).to_owned();
        assert_statement_snapshot!("value-text-every-value-column-is-folded-once", statement);
    }
}
