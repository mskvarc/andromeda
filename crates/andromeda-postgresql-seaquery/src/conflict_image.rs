//! The row image an `ON CONFLICT DO UPDATE` action can read beside the stored
//! row.
//!
//! The action reads the row the statement proposed to insert as `excluded`,
//! and the stored row it updates by the target table's own name. To sea-query
//! `excluded` is an ordinary relation name, so a statement names it by this
//! iden: `Expr::col((Excluded, Table::Column))`.

use sea_query::Iden;

/// The proposed row an `ON CONFLICT DO UPDATE` action reads, `excluded`.
#[derive(Iden, Clone, Copy)]
#[iden = "excluded"]
pub struct Excluded;

#[cfg(test)]
mod tests {
    use crate::conflict_image::Excluded;
    use sea_query::{Alias, Expr, OnConflict, PostgresQueryBuilder, Query};

    #[test]
    fn the_proposed_row_renders_as_the_quoted_pseudo_relation() {
        let mut insert = Query::insert();
        insert.into_table(Alias::new("t")).columns([Alias::new("k"), Alias::new("c")]);
        let sql = insert
            .values([1.into(), 2.into()])
            .expect("the row matches the columns")
            .on_conflict(
                OnConflict::column(Alias::new("k"))
                    .values([(Alias::new("c"), Expr::col((Excluded, Alias::new("c"))))])
                    .to_owned(),
            )
            .to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"ON CONFLICT ("k") DO UPDATE SET "c" = "excluded"."c""#), "{sql}");
    }
}
