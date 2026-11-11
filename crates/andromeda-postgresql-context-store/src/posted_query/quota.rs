//! Keeping a tenant's posted queries within its quota.
//!
//! The live queries are ranked by expiry, the one expiring last first, and
//! every query past the count ceiling, or past the byte ceiling counted along
//! that order, is deleted, never the query just held. Expired queries are left
//! to the sweep: no read serves them, so they take no place in the quota.

use crate::posted_query::lifecycle::live;
use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
use andromeda_posted_query::retention::PostedQueryQuota;
use andromeda_postgresql_schema::posted_query::PostedQueries;
use andromeda_postgresql_seaquery::{aggregate::row_number, catalog_function::column_size};
use sea_query::{Cond, DeleteStatement, Expr, ExprTrait, Func, Iden, Order, Query, SimpleExpr, WindowStatement};

/// The derived table ranking the live queries.
#[derive(Iden, Clone, Copy)]
#[iden = "ranked"]
enum Ranked {
    /// Table alias: `ranked`
    Table,
    /// The query's place in the ranking, counted from one.
    Position,
    /// The bytes the query and every query ranked before it take.
    Footprint,
}

/// The statement deleting the live queries past `quota`, never `held`, or
/// `None` when the quota sets no ceiling.
pub(crate) fn eviction_statement(held: &PageQuery, quota: PostedQueryQuota) -> Option<DeleteStatement> {
    let ceilings: Vec<SimpleExpr> = [
        quota
            .max_count
            .map(|count| Expr::col((Ranked::Table, Ranked::Position)).gt(bigint(count.get()))),
        quota.max_bytes.map(|bytes| Expr::col((Ranked::Table, Ranked::Footprint)).gt(bigint(bytes))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if ceilings.is_empty() {
        return None;
    }
    let past_a_ceiling = ceilings.into_iter().fold(Cond::any(), Cond::add);
    let mut expiring_last_first = WindowStatement::new();
    expiring_last_first
        .order_by(PostedQueries::ExpiresAt, Order::Desc)
        .order_by(PostedQueries::Digest, Order::Desc);
    let ranked = Query::select()
        .column(PostedQueries::Digest)
        .expr_window_as(row_number(), expiring_last_first.clone(), Ranked::Position)
        .expr_window_as(Func::sum(column_size(Expr::col(PostedQueries::Table))), expiring_last_first, Ranked::Footprint)
        .from(PostedQueries::Table)
        .and_where(live())
        .to_owned();
    let victims = Query::select()
        .column((Ranked::Table, PostedQueries::Digest))
        .from_subquery(ranked, Ranked::Table)
        .and_where(Expr::col((Ranked::Table, PostedQueries::Digest)).ne(held.encode()))
        .cond_where(past_a_ceiling)
        .to_owned();
    Some(
        Query::delete()
            .from_table(PostedQueries::Table)
            .and_where(Expr::col(PostedQueries::Digest).in_subquery(victims))
            .to_owned(),
    )
}

/// A ceiling as the `bigint` the ranking counts in. A ceiling past what a
/// `bigint` counts bounds nothing, so it saturates.
fn bigint(ceiling: impl TryInto<i64>) -> i64 {
    ceiling.try_into().unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use crate::posted_query::quota::eviction_statement;
    use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
    use andromeda_posted_query::retention::PostedQueryQuota;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroUsize;

    fn held() -> PageQuery {
        PageQuery::of(Some("type=Held"))
    }

    #[test]
    fn a_quota_without_a_ceiling_deletes_nothing() {
        let quota = PostedQueryQuota {
            max_count: None,
            max_bytes: None,
        };
        assert!(eviction_statement(&held(), quota).is_none());
    }

    /// The ranking counts the held query, so a tenant over its count by one
    /// loses the query expiring soonest, and the held query is never the one
    /// deleted.
    #[test]
    fn the_queries_past_either_ceiling_are_deleted_but_never_the_one_held() {
        let quota = PostedQueryQuota {
            max_count: NonZeroUsize::new(3),
            max_bytes: Some(1_024),
        };
        let sql = eviction_statement(&held(), quota).expect("a ceiling").to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"DELETE FROM "posted_queries" WHERE "digest" IN (SELECT "ranked"."digest" FROM (SELECT "digest", ROW_NUMBER() OVER (  ORDER BY "expires_at" DESC, "digest" DESC ) AS "position", SUM(pg_column_size("posted_queries")) OVER (  ORDER BY "expires_at" DESC, "digest" DESC ) AS "footprint" FROM "posted_queries" WHERE "expires_at" > NOW()) AS "ranked" WHERE "ranked"."digest" <> '{}' AND ("ranked"."position" > 3 OR "ranked"."footprint" > 1024))"#,
                held().encode()
            )
        );
    }

    #[test]
    fn a_count_ceiling_alone_ranks_by_count_alone() {
        let quota = PostedQueryQuota {
            max_count: NonZeroUsize::new(3),
            max_bytes: None,
        };
        let sql = eviction_statement(&held(), quota).expect("a ceiling").to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"AND "ranked"."position" > 3)"#), "{sql}");
    }
}
