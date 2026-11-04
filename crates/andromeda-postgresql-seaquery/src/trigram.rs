//! The trigrams `pg_trgm` extracts from a text: the keys a `gin_trgm_ops`
//! index holds for it.

use sea_query::{Func, Iden, SimpleExpr};

/// The `show_trgm(text)` function of `pg_trgm`.
#[derive(Iden, Clone, Copy)]
#[iden = "show_trgm"]
struct ShowTrgm;

/// `show_trgm(text)`: the distinct trigrams of `text` as a `text[]`, one
/// element per key a `gin_trgm_ops` index holds for it. NULL for a NULL text.
#[must_use]
pub fn show_trgm(text: SimpleExpr) -> SimpleExpr {
    Func::cust(ShowTrgm).arg(text).into()
}

#[cfg(test)]
mod tests {
    use crate::trigram::show_trgm;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_trigrams_of_a_column_are_read_with_show_trgm() {
        let sql = Query::select()
            .expr(show_trgm(Expr::col(Alias::new("id"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT show_trgm("id")"#);
    }
}
