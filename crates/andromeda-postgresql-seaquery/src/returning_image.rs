//! The row images a PostgreSQL 18 `RETURNING` clause can read.
//!
//! From PostgreSQL 18 on, the `RETURNING` list of an `INSERT`, `UPDATE`,
//! `DELETE` or `MERGE` may qualify a column with `old` or `new` to read the row
//! as it stood before the statement changed it or as the statement left it. An
//! `old` column is NULL for a row the statement inserted, a `new` column NULL
//! for a row it deleted. They are ordinary relation names to sea-query, so a
//! statement names them by these idens: `Expr::col((Old, Table::Column))`.

use sea_query::Iden;

/// The pre-image a `RETURNING` clause reads, `old`.
#[derive(Iden, Clone, Copy)]
#[iden = "old"]
pub struct Old;

/// The post-image a `RETURNING` clause reads, `new`.
#[derive(Iden, Clone, Copy)]
#[iden = "new"]
pub struct New;

#[cfg(test)]
mod tests {
    use crate::returning_image::{New, Old};
    use sea_query::{Expr, Iden, PostgresQueryBuilder, Query};

    #[test]
    fn the_images_render_as_the_quoted_pseudo_relations() {
        assert_eq!(Old.to_string(), "old");
        assert_eq!(New.to_string(), "new");
        let sql = Query::update()
            .table("t")
            .value("c", 1)
            .returning(Query::returning().exprs([Expr::col((Old, "c")), Expr::col((New, "c"))]))
            .to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"RETURNING "old"."c", "new"."c""#), "{sql}");
    }
}
