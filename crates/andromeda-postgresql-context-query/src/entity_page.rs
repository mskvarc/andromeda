//! The first stage of a paginated entity query: the page is cut from the
//! matching entities on its own, and the head of each Entity it kept is read
//! for the rows it kept. The Entities' `current_attributes` rows are the second
//! stage, read by the keys this one returns.
//!
//! PostgreSQL evaluates a statement's select list below its `LIMIT`, so a
//! column projected beside `LIMIT n OFFSET m` is read for all `m + n` rows the
//! window walks, not for the `n` it returns. The page therefore projects the
//! entity key, the total and its sort keys, and the statement around it looks
//! each kept row's `entities` and `entity_search` rows up by key, reading the
//! head ([`EntityHead`]), which carries the clause 4.8 system attributes a
//! `sysAttrs` answer writes. The lookup is a lateral subquery capped at one
//! row, which PostgreSQL never flattens into a join of its own: it runs once
//! per kept row on the two primary keys, never as a scan of either relation.
//! Rows read out of a derived table have no order of their own, so the outer
//! statement restates the page's order on the columns the page projected its
//! keys under: clause 4.23.1 returns the entities sorted, and clause 5.5.9
//! pages over that order.

use crate::entity_head::{EntityHead, select_head};
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    sort_key::{KeyOrder, SortKey},
};
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{aggregate::count_over_all, branch::qualified};
use derive_more::TryFrom;
use sea_query::{Alias, Condition, Expr, ExprTrait, JoinType, Query, SelectStatement};

/// Column name the total of a counted query is read back under.
///
/// The page counts every matching entity as `COUNT(*) OVER ()`, which
/// PostgreSQL evaluates over the whole unbounded partition before the window
/// cuts the page, and the head statement carries it through to each row.
/// The entity repository reads it back under this name, so the two have to
/// agree; it is a constant rather than a literal for exactly that reason.
pub const TOTAL_COUNT_COLUMN: &str = "total_count";

/// Column name a gated page's rows name the step that answered it under.
pub const GATED_STEP_COLUMN: &str = "gated_step";

/// The step of a gated page that answered it, as its rows name it under
/// [`GATED_STEP_COLUMN`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFrom)]
#[try_from(repr)]
#[repr(i16)]
pub enum GatedStep {
    /// The short walk filled the page: the matches are dense where the
    /// selection starts.
    ShortWalk = 1,
    /// The candidates numbered at most the bound and the page was cut from
    /// them.
    Candidates = 2,
    /// More candidates qualified and the selection was walked.
    FullWalk = 3,
}

/// Whether a page's rows name the step of a gated page that answered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepColumn {
    /// Each row carries the step under [`GATED_STEP_COLUMN`].
    Named,
    /// The rows carry no step.
    Unnamed,
}

/// The name the head statement reads the page under.
const PAGE: &str = "page";

/// The name the lookup reads each kept row's search row under.
const SEARCH: &str = "s";

/// The name the lookup reads each kept row's identity row under.
const IDENTITY: &str = "e";

/// The name the head statement reads one kept row's lookup under.
const LOOKUP: &str = "stored";

/// Read the heads of the entities `page` keeps, in the order `keys` sorts
/// them.
///
/// `page` is the filtered, windowed statement over the entity rows `rows`
/// names, carrying its `FROM`, its `WHERE`, its `LIMIT` and `OFFSET` or keyset
/// predicate, and any lateral its keys read, but no projection and no `ORDER
/// BY`: both are added here, because the page is ordered by the columns it
/// projects its keys under and the head statement by the same columns read
/// back through the page.
#[must_use]
pub fn heads_of(page: SelectStatement, rows: &EntityRowRefs, keys: Vec<SortKey>, count: CountPolicy) -> SelectStatement {
    heads_over(projected_page(page, rows, keys, count), rows, count, StepColumn::Unnamed)
}

/// A page projecting its entity key and its sort keys, and the order it is
/// read in.
#[derive(Debug, Clone)]
pub struct ProjectedPage {
    /// The page statement.
    pub select: SelectStatement,
    /// Each sort key's column and direction, in the order they decide.
    pub order: Vec<(Alias, KeyOrder)>,
}

/// Project `page`'s entity key, its total when `count` asks for it, and each
/// of `keys` under `sort_<n>`, and order it by those columns.
#[must_use]
pub fn projected_page(mut page: SelectStatement, rows: &EntityRowRefs, keys: Vec<SortKey>, count: CountPolicy) -> ProjectedPage {
    page.expr(rows.entity_key_column());
    match count {
        CountPolicy::Emit => {
            page.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
        }
        CountPolicy::Omit => {}
    }
    let order = keys
        .into_iter()
        .enumerate()
        .map(|(position, key)| {
            let column = sort_column(position);
            let (expression, order) = key.into_parts();
            // clone: the page projects the key under this name and orders by it.
            page.expr_as(expression, column.clone());
            order.order_by(&mut page, Expr::col(column.clone()));
            (column, order)
        })
        .collect();
    ProjectedPage { select: page, order }
}

/// The column a page projects its key in `position` under.
#[must_use]
pub fn sort_column(position: usize) -> Alias {
    Alias::new(format!("sort_{position}"))
}

/// Read the head of every Entity a projected `page` keeps, in its order,
/// with the total `count` says the page carries and the gated step `step`
/// says it names.
#[must_use]
pub fn heads_over(page: ProjectedPage, rows: &EntityRowRefs, count: CountPolicy, step: StepColumn) -> SelectStatement {
    let ProjectedPage { select: page, order } = page;
    let mut heads = Query::select();
    // The page carries no search row of its own beyond what its filters
    // joined; the head is read by the key the page projected.
    for member in EntityHead::ALL {
        heads.column((Alias::new(LOOKUP), member));
    }
    match count {
        CountPolicy::Emit => {
            heads.expr_as(Expr::col((Alias::new(PAGE), Alias::new(TOTAL_COUNT_COLUMN))), Alias::new(TOTAL_COUNT_COLUMN));
        }
        CountPolicy::Omit => {}
    }
    match step {
        StepColumn::Named => {
            heads.expr_as(Expr::col((Alias::new(PAGE), Alias::new(GATED_STEP_COLUMN))), Alias::new(GATED_STEP_COLUMN));
        }
        StepColumn::Unnamed => {}
    }
    heads
        .from_subquery(page, Alias::new(PAGE))
        .join_lateral(JoinType::InnerJoin, stored_head_lookup(rows), Alias::new(LOOKUP), Condition::all());
    for (column, key_order) in order {
        key_order.order_by(&mut heads, Expr::col((Alias::new(PAGE), column)));
    }
    heads.take()
}

/// The identity and search rows of the page row the lookup is lateral to,
/// read by their shared key.
///
/// The `LIMIT 1` is what keeps the lookup a per-row probe: PostgreSQL pulls a
/// plain subquery up into the join above it, where the planner may hash the
/// whole relation for a hundred kept rows, but never one that limits its rows.
/// The key is unique, so the limit drops nothing.
fn stored_head_lookup(rows: &EntityRowRefs) -> SelectStatement {
    let (identity, search) = (Alias::new(IDENTITY), Alias::new(SEARCH));
    let mut lookup = Query::select();
    select_head(&mut lookup, identity.clone(), search.clone());
    lookup
        .from_as(qualified(rows.schema(), Entities::Table), identity.clone())
        .join_as(
            JoinType::InnerJoin,
            qualified(rows.schema(), EntitySearch::Table),
            search.clone(),
            Expr::col((search, EntitySearch::EntityKey)).equals((identity.clone(), Entities::EntityKey)),
        )
        .and_where(Expr::col((identity, Entities::EntityKey)).equals((Alias::new(PAGE), Entities::EntityKey)))
        .limit(1)
        .take()
}

#[cfg(test)]
mod tests {
    use crate::entity_page::heads_of;
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, ordering::sort_direction::SortDirection};
    use andromeda_postgresql_query::{
        entity_row_refs::EntityRowRefs,
        sort_key::{KeyOrder, NullPlacement, SortKey},
    };
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Query, SelectStatement};

    fn windowed_page() -> SelectStatement {
        Query::select().from(Entities::Table).limit(100).offset(5000).take()
    }

    fn by_id(rows: &EntityRowRefs) -> Vec<SortKey> {
        vec![SortKey::new(
            rows.identity_column(Entities::Id),
            KeyOrder::new(SortDirection::Ascending, NullPlacement::Unstated),
        )]
    }

    /// The heads are read for the page's rows alone, outside the window, and
    /// the page's order is restated on the column it projected its key under.
    #[test]
    fn the_heads_are_read_outside_the_window_in_the_pages_order() {
        let rows = EntityRowRefs::unaliased(None);
        let sql = heads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "stored"."entity_key", "stored"."id", "stored"."entity_created_at", "stored"."entity_modified_at", "stored"."entity_write_count", "stored"."entity_expires_at", "#,
                r#""stored"."entity_type_name_keys", "stored"."verbatim_scope_paths" "#,
                r#"FROM (SELECT "entities"."entity_key", "entities"."id" AS "sort_0" FROM "entities" ORDER BY "sort_0" ASC LIMIT 100 OFFSET 5000) AS "page" "#,
                r#"INNER JOIN LATERAL (SELECT "e"."entity_key" AS "entity_key", "e"."id" AS "id", "e"."created_at" AS "entity_created_at", "#,
                r#""e"."modified_at" AS "entity_modified_at", "e"."write_count" AS "entity_write_count", "e"."expires_at" AS "entity_expires_at", "#,
                r#""s"."entity_type_name_keys" AS "entity_type_name_keys", "s"."verbatim_scope_paths" AS "verbatim_scope_paths" "#,
                r#"FROM "entities" AS "e" INNER JOIN "entity_search" AS "s" ON "s"."entity_key" = "e"."entity_key" "#,
                r#"WHERE "e"."entity_key" = "page"."entity_key" LIMIT 1) AS "stored" ON TRUE "#,
                r#"ORDER BY "page"."sort_0" ASC"#
            )
        );
    }

    /// The total counts the matching entities inside the page, before the
    /// window cuts it, and is carried out under the name the store reads.
    #[test]
    fn the_total_is_counted_inside_the_page_and_carried_out() {
        let rows = EntityRowRefs::unaliased(None);
        let sql = heads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Emit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""page"."total_count" AS "total_count""#), "{sql}");
        assert!(sql.contains(r#"COUNT(*) OVER () AS "total_count""#), "{sql}");
    }

    /// Every key keeps its direction and its null placement at both levels.
    #[test]
    fn each_key_is_restated_with_its_direction_and_null_placement() {
        let rows = EntityRowRefs::unaliased(None);
        let keys = vec![
            SortKey::new(
                rows.identity_column(Entities::CreatedAt),
                KeyOrder::new(SortDirection::Descending, NullPlacement::Last),
            ),
            SortKey::new(
                rows.identity_column(Entities::Id),
                KeyOrder::new(SortDirection::Ascending, NullPlacement::Unstated),
            ),
        ];
        let sql = heads_of(windowed_page(), &rows, keys, CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"ORDER BY "sort_0" DESC NULLS LAST, "sort_1" ASC LIMIT"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "page"."sort_0" DESC NULLS LAST, "page"."sort_1" ASC"#), "{sql}");
    }

    /// A Snapshot branch's heads are looked up in the branch's own schema.
    #[test]
    fn a_branch_page_looks_its_heads_up_in_the_branch_schema() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snap_5").into_iden()));
        let sql = heads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_5"."entities" AS "e""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "snap_5"."entity_search" AS "s""#), "{sql}");
    }
}
