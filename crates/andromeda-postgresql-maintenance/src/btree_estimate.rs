//! A catalog estimate of the free space in each probeable B-tree.
//!
//! `pgstattuple` reads every page of the index it measures. The estimate reads
//! only `pg_class` and `pg_stats`: the pages the index's tuples would fill
//! packed at the default leaf fillfactor, against the pages it holds. It
//! ignores deduplication, which shrinks an index whose leading key repeats, as
//! the attribute key does in every probeable B-tree, so it understates bloat.
//! It orders probes and never decides whether an index is bloated.

use crate::{
    branch::Branch,
    candidate::index_target::IndexTarget,
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{cast::cast, introspection::current_setting, pg_type::PostgresType, setting::Setting, statement::Statement};
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Iden, JoinType, Query, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::{num::NonZeroU64, str::FromStr};

/// Leaf fillfactor a B-tree is built with when its definition sets none.
const BTREE_FILLFACTOR_PERCENT: u64 = 90;

/// `PageHeaderData` bytes at the start of every page.
const PAGE_HEADER_BYTES: u64 = 24;

/// `BTPageOpaqueData` bytes at the end of every B-tree page.
const BTREE_SPECIAL_BYTES: u64 = 16;

/// `IndexTupleData` bytes ahead of each tuple's key.
const INDEX_TUPLE_HEADER_BYTES: u64 = 8;

/// `ItemIdData` bytes each tuple takes in its page's line pointer array.
const LINE_POINTER_BYTES: u64 = 4;

/// `MAXALIGN` on the 64-bit platforms PostgreSQL runs on: each tuple is padded
/// to it.
const MAXIMUM_ALIGNMENT: u64 = 8;

/// The catalog facts one B-tree's estimate is computed from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BtreeLayout {
    /// Index entries (`pg_class.reltuples` of the index).
    pub(crate) tuples: u64,
    /// Sum of the indexed columns' average widths in bytes (`pg_stats.avg_width`).
    pub(crate) key_width: u64,
    /// Pages the index holds (`pg_class.relpages` of the index).
    pub(crate) pages: u64,
    /// The server's page size in bytes (`block_size`).
    pub(crate) block_size: u64,
}

impl BtreeLayout {
    /// Percent of the index's pages its tuples would not need, packed at the
    /// default leaf fillfactor. `None` when the index holds no pages or a page
    /// has no room for tuples.
    pub(crate) fn free_percent(self) -> Option<u8> {
        if self.pages == 0 {
            return None;
        }
        let tuple_bytes = INDEX_TUPLE_HEADER_BYTES
            .saturating_add(self.key_width)
            .next_multiple_of(MAXIMUM_ALIGNMENT)
            .saturating_add(LINE_POINTER_BYTES);
        let page_bytes = self.block_size.checked_sub(PAGE_HEADER_BYTES + BTREE_SPECIAL_BYTES)?;
        let usable_bytes = NonZeroU64::new(page_bytes.saturating_mul(BTREE_FILLFACTOR_PERCENT) / 100)?;
        let expected_pages = self.tuples.saturating_mul(tuple_bytes).div_ceil(usable_bytes.get());
        Some(percent(self.pages.saturating_sub(expected_pages), self.pages))
    }
}

/// One probeable B-tree's estimated free space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BtreeEstimate {
    /// The estimated index.
    pub(crate) target: IndexTarget,
    /// Estimated free space, percent of its pages.
    pub(crate) free_percent: u8,
}

/// Estimate every probeable B-tree of `branch` whose layout the catalogs
/// fully describe.
///
/// An index never analyzed (`reltuples` of -1), or with an indexed column
/// `pg_stats` does not describe, has no estimate.
pub(crate) async fn btree_estimates(connection: &mut PgConnection, branch: Branch) -> Result<Vec<BtreeEstimate>, MaintenanceError> {
    let rows = btree_layouts_statement(branch).to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let name = row.try_get::<String, _>("relname")?;
            let target = IndexTarget::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            Ok(layout_of(row)?.free_percent().map(|free_percent| BtreeEstimate { target, free_percent }))
        })
        .filter_map(Result::transpose)
        .collect()
}

/// The columns the layout statement projects.
#[derive(Clone, Copy, Iden)]
enum LayoutColumn {
    /// `pg_class.relpages`.
    Pages,
    /// `pg_class.reltuples`.
    Tuples,
    /// Sum of `pg_stats.avg_width` over the indexed columns.
    KeyWidth,
    /// `block_size`.
    BlockSize,
}

/// A column of one aliased catalog.
fn catalog_column(alias: &'static str, column: &'static str) -> Expr {
    Expr::col((Alias::new(alias), Alias::new(column)))
}

/// Every probeable B-tree in `branch`'s schema with its pages, entries and key
/// width, its columns' widths read from its own heap's statistics.
///
/// The `having` keeps only indexes whose every column has a `pg_stats` row:
/// the inner join drops a column without one, and its width would be missing
/// from the sum.
fn btree_layouts_statement(branch: Branch) -> SelectStatement {
    let probeable = IndexTarget::PROBEABLE.map(IndexTarget::relation_name);
    Query::select()
        .column((Alias::new("c"), Alias::new("relname")))
        .expr_as(cast(catalog_column("c", "relpages"), PostgresType::Bigint), LayoutColumn::Pages)
        .expr_as(cast(catalog_column("c", "reltuples"), PostgresType::Bigint), LayoutColumn::Tuples)
        .expr_as(
            cast(Func::sum(catalog_column("s", "avg_width")).into(), PostgresType::Bigint),
            LayoutColumn::KeyWidth,
        )
        .expr_as(cast(current_setting(Setting::BlockSize), PostgresType::Bigint), LayoutColumn::BlockSize)
        .from_as(Alias::new("pg_class"), Alias::new("c"))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_namespace"),
            Alias::new("n"),
            catalog_column("n", "oid").equals((Alias::new("c"), Alias::new("relnamespace"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_am"),
            Alias::new("am"),
            catalog_column("am", "oid").equals((Alias::new("c"), Alias::new("relam"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_index"),
            Alias::new("i"),
            catalog_column("i", "indexrelid").equals((Alias::new("c"), Alias::new("oid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new("t"),
            catalog_column("t", "oid").equals((Alias::new("i"), Alias::new("indrelid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_attribute"),
            Alias::new("a"),
            catalog_column("a", "attrelid").equals((Alias::new("c"), Alias::new("oid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_stats"),
            Alias::new("s"),
            catalog_column("s", "schemaname")
                .equals((Alias::new("n"), Alias::new("nspname")))
                .and(catalog_column("s", "tablename").equals((Alias::new("t"), Alias::new("relname"))))
                .and(catalog_column("s", "attname").equals((Alias::new("a"), Alias::new("attname"))))
                .and(catalog_column("s", "inherited").eq(false)),
        )
        .and_where(catalog_column("n", "nspname").eq(branch.schema_name()))
        .and_where(catalog_column("am", "amname").eq("btree"))
        .and_where(catalog_column("c", "relname").is_in(probeable))
        .and_where(catalog_column("c", "reltuples").gte(0))
        .group_by_columns([
            (Alias::new("c"), Alias::new("relname")),
            (Alias::new("c"), Alias::new("relpages")),
            (Alias::new("c"), Alias::new("reltuples")),
            (Alias::new("c"), Alias::new("relnatts")),
        ])
        .and_having(Expr::from(Func::count(Expr::col(Asterisk))).eq(catalog_column("c", "relnatts")))
        .to_owned()
}

/// Decode one layout row.
fn layout_of(row: &PgRow) -> Result<BtreeLayout, MaintenanceError> {
    let counter =
        |column: LayoutColumn, kind: CatalogCounter| -> Result<u64, MaintenanceError> { non_negative(row.try_get(column.to_string().as_str())?, kind) };
    Ok(BtreeLayout {
        tuples: counter(LayoutColumn::Tuples, CatalogCounter::IndexTuples)?,
        key_width: counter(LayoutColumn::KeyWidth, CatalogCounter::IndexKeyWidth)?,
        pages: counter(LayoutColumn::Pages, CatalogCounter::IndexPages)?,
        block_size: counter(LayoutColumn::BlockSize, CatalogCounter::BlockSize)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        btree_estimate::{BtreeLayout, btree_layouts_statement},
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// Each tuple takes `MAXALIGN(8 + 12) + 4 = 28` bytes; a page holds
    /// `(8192 - 24 - 16) * 90 / 100 = 7336` of them; 1,000,000 tuples need
    /// `ceil(28,000,000 / 7336) = 3817` pages; `(10,000 - 3817) * 100 / 10,000`
    /// is 61 percent.
    #[test]
    fn btree_estimate_matches_a_known_layout() {
        let layout = BtreeLayout {
            tuples: 1_000_000,
            key_width: 12,
            pages: 10_000,
            block_size: 8_192,
        };
        assert_eq!(layout.free_percent(), Some(61));
    }

    #[test]
    fn a_packed_or_empty_btree_estimates_no_free_space_or_none() {
        let packed = BtreeLayout {
            tuples: 1_000_000,
            key_width: 12,
            pages: 3_817,
            block_size: 8_192,
        };
        assert_eq!(packed.free_percent(), Some(0));
        let overfull = BtreeLayout { pages: 1_000, ..packed };
        assert_eq!(overfull.free_percent(), Some(0));
        let empty = BtreeLayout { pages: 0, ..packed };
        assert_eq!(empty.free_percent(), None);
    }

    #[test]
    fn the_layout_statement_reads_only_probeable_btrees_with_complete_statistics() {
        let sql = btree_layouts_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n"."nspname" = 'public'"#), "{sql}");
        assert!(sql.contains(r#"JOIN "pg_stats" AS "s""#), "{sql}");
        assert!(sql.contains(r#"JOIN "pg_class" AS "t" ON "t"."oid" = "i"."indrelid""#), "{sql}");
        assert!(
            sql.contains(r#""s"."tablename" = "t"."relname""#),
            "an index reads its own heap's statistics: {sql}"
        );
        assert!(!sql.contains("'current_attributes'"), "{sql}");
        assert!(sql.contains(r#""am"."amname" = 'btree'"#), "{sql}");
        assert!(sql.contains(r#""c"."reltuples" >= 0"#), "an index never analyzed has no estimate: {sql}");
        assert!(sql.contains(r#"HAVING COUNT(*) = "c"."relnatts""#), "{sql}");
        assert!(sql.contains("current_setting('block_size')::bigint"), "{sql}");
        assert!(
            sql.contains("'idx_ca_value_geometry'"),
            "the access method, not the name list, excludes the GiST: {sql}"
        );
        assert!(!sql.contains("'idx_ca_value_string_trgm'"), "{sql}");
    }
}
