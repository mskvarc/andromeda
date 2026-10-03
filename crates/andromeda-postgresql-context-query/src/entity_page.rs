//! The payload statement of a paginated entity query: the page is cut from
//! the matching entities on its own, and the payloads are read for the rows it
//! kept.
//!
//! PostgreSQL evaluates a statement's select list below its `LIMIT`, so a
//! payload projected beside `LIMIT n OFFSET m` is read, and de-TOASTed, for all
//! `m + n` rows the window walks, not for the `n` it returns. The page
//! therefore projects the entity key, the total and its sort keys, and the
//! statement around it looks each kept row's document and identity rows up by
//! key, reading the payload together with the clause 4.8 system attributes a
//! `sysAttrs` answer writes beside it. The lookup is a lateral subquery capped
//! at one row, which PostgreSQL never flattens into a join of its own: it runs
//! once per kept row on the two primary keys, never as a scan of either
//! relation. Rows read out of a derived table have
//! no order of their own, so the outer statement restates the page's order on
//! the columns the page projected its keys under: clause 4.23.1 returns the
//! entities sorted, and clause 5.5.9 pages over that order.

use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_postgresql_query::{entity_row_refs::EntityRowRefs, sort_key::SortKey};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments};
use andromeda_postgresql_seaquery::{aggregate::count_over_all, branch::qualified};
use sea_query::{Alias, Condition, Expr, ExprTrait, JoinType, Query, SelectStatement};

/// Column name the total of a counted query is read back under.
///
/// The page counts every matching entity as `COUNT(*) OVER ()`, which
/// PostgreSQL evaluates over the whole unbounded partition before the window
/// cuts the page, and the payload statement carries it through to each row.
/// The entity repository reads it back under this name, so the two have to
/// agree; it is a constant rather than a literal for exactly that reason.
pub const TOTAL_COUNT_COLUMN: &str = "total_count";

/// The name the payload statement reads the page under.
const PAGE: &str = "page";

/// The name the lookup reads each kept row's document row under.
const DOCUMENT: &str = "d";

/// The name the lookup reads each kept row's identity row under.
const IDENTITY: &str = "e";

/// The name the payload statement reads one kept row's lookup under.
const LOOKUP: &str = "stored";

/// Read the payloads of the entities `page` keeps, with their `created_at`,
/// `modified_at` and `expires_at`, in the order `keys` sorts them.
///
/// `page` is the filtered, windowed statement over the entity rows `rows`
/// names, carrying its `FROM`, its `WHERE`, its `LIMIT` and `OFFSET` or keyset
/// predicate, and any lateral its keys read, but no projection and no `ORDER
/// BY`: both are added here, because the page is ordered by the columns it
/// projects its keys under and the payload statement by the same columns read
/// back through the page.
#[must_use]
pub fn payloads_of(mut page: SelectStatement, rows: &EntityRowRefs, keys: Vec<SortKey>, count: CountPolicy) -> SelectStatement {
    page.expr(rows.entity_key_column());
    match count {
        CountPolicy::Emit => {
            page.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
        }
        CountPolicy::Omit => {}
    }

    let mut payloads = Query::select();
    // The page carries no search row and no document row of its own; the
    // payload and the system attributes are read by the key the page
    // projected, and nothing here reads a search column.
    payloads
        .column((Alias::new(LOOKUP), EntityDocuments::Payload))
        .column((Alias::new(LOOKUP), Entities::CreatedAt))
        .column((Alias::new(LOOKUP), EntityDocuments::ModifiedAt))
        .column((Alias::new(LOOKUP), Entities::ExpiresAt));
    match count {
        CountPolicy::Emit => {
            payloads.expr_as(Expr::col((Alias::new(PAGE), Alias::new(TOTAL_COUNT_COLUMN))), Alias::new(TOTAL_COUNT_COLUMN));
        }
        CountPolicy::Omit => {}
    }

    let restated: Vec<_> = keys
        .into_iter()
        .enumerate()
        .map(|(position, key)| {
            let column = Alias::new(format!("sort_{position}"));
            let (expression, order) = key.into_parts();
            // clone: the page projects the key under this name and orders by it.
            page.expr_as(expression, column.clone());
            order.order_by(&mut page, Expr::col(column.clone()));
            (column, order)
        })
        .collect();

    payloads
        .from_subquery(page, Alias::new(PAGE))
        .join_lateral(JoinType::InnerJoin, stored_row_lookup(rows), Alias::new(LOOKUP), Condition::all());
    for (column, order) in restated {
        order.order_by(&mut payloads, Expr::col((Alias::new(PAGE), column)));
    }
    payloads.take()
}

/// The document and identity rows of the page row the lookup is lateral to,
/// read by their shared key.
///
/// The `LIMIT 1` is what keeps the lookup a per-row probe: PostgreSQL pulls a
/// plain subquery up into the join above it, where the planner may hash the
/// whole relation for a hundred kept rows, but never one that limits its rows.
/// The key is unique, so the limit drops nothing.
fn stored_row_lookup(rows: &EntityRowRefs) -> SelectStatement {
    let (document, identity) = (Alias::new(DOCUMENT), Alias::new(IDENTITY));
    Query::select()
        .column((document.clone(), EntityDocuments::Payload))
        .column((identity.clone(), Entities::CreatedAt))
        .column((document.clone(), EntityDocuments::ModifiedAt))
        .column((identity.clone(), Entities::ExpiresAt))
        .from_as(qualified(rows.schema(), EntityDocuments::Table), document.clone())
        .join_as(
            JoinType::InnerJoin,
            qualified(rows.schema(), Entities::Table),
            identity.clone(),
            Expr::col((identity, Entities::EntityKey)).equals((document.clone(), EntityDocuments::EntityKey)),
        )
        .and_where(Expr::col((document, EntityDocuments::EntityKey)).equals((Alias::new(PAGE), Entities::EntityKey)))
        .limit(1)
        .take()
}

#[cfg(test)]
mod tests {
    use crate::entity_page::payloads_of;
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

    /// The payload and the system attributes are read for the page's rows
    /// alone, outside the window, and the page's order is restated on the
    /// column it projected its key under.
    #[test]
    fn the_payload_is_read_outside_the_window_in_the_pages_order() {
        let rows = EntityRowRefs::unaliased(None);
        let sql = payloads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "stored"."payload", "stored"."created_at", "stored"."modified_at", "stored"."expires_at" "#,
                r#"FROM (SELECT "entities"."entity_key", "entities"."id" AS "sort_0" FROM "entities" ORDER BY "sort_0" ASC LIMIT 100 OFFSET 5000) AS "page" "#,
                r#"INNER JOIN LATERAL (SELECT "d"."payload", "e"."created_at", "d"."modified_at", "e"."expires_at" FROM "entity_documents" AS "d" "#,
                r#"INNER JOIN "entities" AS "e" ON "e"."entity_key" = "d"."entity_key" WHERE "d"."entity_key" = "page"."entity_key" LIMIT 1) AS "stored" ON TRUE "#,
                r#"ORDER BY "page"."sort_0" ASC"#
            )
        );
    }

    /// The total counts the matching entities inside the page, before the
    /// window cuts it, and is carried out under the name the store reads.
    #[test]
    fn the_total_is_counted_inside_the_page_and_carried_out() {
        let rows = EntityRowRefs::unaliased(None);
        let sql = payloads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Emit).to_string(PostgresQueryBuilder);
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
        let sql = payloads_of(windowed_page(), &rows, keys, CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"ORDER BY "sort_0" DESC NULLS LAST, "sort_1" ASC LIMIT"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "page"."sort_0" DESC NULLS LAST, "page"."sort_1" ASC"#), "{sql}");
    }

    /// A Snapshot branch's payloads are looked up in the branch's own schema.
    #[test]
    fn a_branch_page_looks_its_payloads_up_in_the_branch_schema() {
        let rows = EntityRowRefs::unaliased(Some(Alias::new("snap_5").into_iden()));
        let sql = payloads_of(windowed_page(), &rows, by_id(&rows), CountPolicy::Omit).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_5"."entity_documents" AS "d""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "snap_5"."entities" AS "e""#), "{sql}");
    }
}
