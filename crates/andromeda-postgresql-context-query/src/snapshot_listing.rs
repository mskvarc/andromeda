//! The statements that list the entities a materialised snapshot holds
//! (clause 5.16, Snapshot Functionality).
//!
//! The runtime table and column names come from the caller because the same
//! shape serves both the entity snapshot table and the temporal snapshot table;
//! they are trusted identifiers the repository supplies, never request input.
//! Both child tables are keyed by the snapshot's surrogate, which every
//! statement here resolves from the URI through
//! [`crate::snapshot_key::snapshot_key_of`].

use crate::snapshot_key::snapshot_key_of;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_schema::snapshot::SnapshotEntities;
use andromeda_postgresql_seaquery::{aggregate::count_all, literal::text_literal, pg_array::any, pg_operator::PgBinaryOperator};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Iden, Order, Query, SelectStatement, SimpleExpr};

/// The inputs that select one page of a snapshot's entities.
#[derive(Debug)]
pub struct SnapshotListingParams<'a> {
    /// The table the snapshot's rows live in (e.g. `snapshot_entities`).
    pub table: &'a str,
    /// The JSONB column holding each entity's representation (e.g. `entity_data`).
    pub data_column: &'a str,
    /// The snapshot whose entities are listed.
    pub snapshot_id: &'a Uri,
    /// Restrict to entities of this NGSI-LD type, when the request names one.
    pub entity_type: Option<&'a TypeExprRaw>,
    /// Restrict to these entity identifiers, when the request names any.
    pub entity_ids: Option<&'a [Uri]>,
    /// Page size (clause 5.5.9 pagination).
    pub limit: i64,
    /// Page offset (clause 5.5.9 pagination).
    pub offset: i64,
}

/// The paginated data statement and its matching total-count statement.
///
/// The two share one `WHERE` condition so the count reports exactly the rows the
/// page draws from; each is executed on its own through `build_sqlx`.
pub struct SnapshotListing {
    /// The `SELECT <data> … LIMIT … OFFSET …` page.
    pub data: SelectStatement,
    /// The `SELECT COUNT(*) …` over the same filter, without pagination.
    pub count: SelectStatement,
}

/// The shared `WHERE` condition selecting a snapshot's matching entities.
fn listing_condition(params: &SnapshotListingParams<'_>) -> Condition {
    let mut condition = Condition::all().add(SeaExpr::col(SnapshotEntities::SnapshotKey).eq(snapshot_key_of(SeaExpr::val(params.snapshot_id.as_str()))));

    if let Some(entity_type) = params.entity_type {
        // The stored representation carries the type as a JSON member; a
        // substring match keeps the previous `LIKE '%type%'` behaviour, with the
        // wildcarded pattern bound as one value.
        let stored_type = SimpleExpr::from(SeaExpr::col(Alias::new(params.data_column))).json_get_text(text_literal("type"));
        condition = condition.add(SeaExpr::expr(stored_type).like(format!("%{entity_type}%")));
    }

    if let Some(ids) = params.entity_ids
        && !ids.is_empty()
    {
        let ids: Vec<String> = ids.iter().map(Uri::to_string).collect();
        condition = condition.add(SeaExpr::col(SnapshotEntities::EntityId).eq(any(SeaExpr::val(ids))));
    }

    condition
}

/// Build the count-and-data statement pair that lists a snapshot's entities.
pub fn build_listing_query(params: &SnapshotListingParams<'_>) -> SnapshotListing {
    // clone: the count statement filters on the same condition.
    let condition = listing_condition(params);

    let mut data = Query::select();
    data.expr(SeaExpr::col(Alias::new(params.data_column)))
        .from(Alias::new(params.table))
        .cond_where(condition.clone())
        .order_by(SnapshotEntities::EntityId, Order::Asc)
        .limit(<i64 as Ord>::max(params.limit, 0) as u64)
        .offset(<i64 as Ord>::max(params.offset, 0) as u64);

    let mut count = Query::select();
    count
        .expr_as(count_all(), Alias::new("cnt"))
        .from(Alias::new(params.table))
        .cond_where(condition);

    SnapshotListing { data, count }
}

/// Build the statement that fetches one entity's representation from a snapshot.
///
/// `table` and `data_column` are the SQL identifiers the repository supplies,
/// named through [`Iden`] so a rename moves the reference with them; the snapshot
/// and the entity are named by their domain identifiers.
pub fn build_get_row_query(table: impl Iden + 'static, data_column: impl Iden + 'static, snapshot_id: &Uri, entity_id: &Uri) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr(SeaExpr::col(data_column))
        .from(table)
        .and_where(SeaExpr::col(SnapshotEntities::SnapshotKey).eq(snapshot_key_of(SeaExpr::val(snapshot_id.as_str()))))
        .and_where(SeaExpr::col(SnapshotEntities::EntityId).eq(entity_id.as_str()));
    select
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn params<'a>(snapshot_id: &'a Uri, entity_type: Option<&'a TypeExprRaw>, entity_ids: Option<&'a [Uri]>) -> SnapshotListingParams<'a> {
        SnapshotListingParams {
            table: "snapshot_entities",
            data_column: "entity_data",
            snapshot_id,
            entity_type,
            entity_ids,
            limit: 25,
            offset: 50,
        }
    }

    #[test]
    fn the_bare_listing_filters_only_by_snapshot_and_pages() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let listing = build_listing_query(&params(&snapshot_id, None, None));
        let data = listing.data.to_string(PostgresQueryBuilder);
        assert!(data.contains("SELECT \"entity_data\" FROM \"snapshot_entities\""), "{data}");
        assert!(
            data.contains("\"snapshot_key\" = (SELECT \"snapshots\".\"snapshot_key\" FROM \"snapshots\" WHERE \"snapshots\".\"id\" = "),
            "{data}"
        );
        assert!(data.contains("ORDER BY \"entity_id\" ASC"), "{data}");
        assert!(data.contains("LIMIT 25 OFFSET 50"), "{data}");
        let count = listing.count.to_string(PostgresQueryBuilder);
        assert!(count.contains("COUNT(*)"), "{count}");
        assert!(!count.contains("LIMIT"), "{count}");
    }

    #[test]
    fn a_type_filter_matches_the_json_type_member() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let entity_type = "Vehicle".parse().expect("valid type expression");
        let listing = build_listing_query(&params(&snapshot_id, Some(&entity_type), None));
        for sql in [listing.data.to_string(PostgresQueryBuilder), listing.count.to_string(PostgresQueryBuilder)] {
            assert!(sql.contains("\"entity_data\" ->> ('type')"), "{sql}");
            assert!(sql.contains(" LIKE "), "{sql}");
            assert!(sql.contains("'%Vehicle%'"), "{sql}");
        }
    }

    #[test]
    fn an_id_filter_renders_an_array_membership_test() {
        let ids = vec!["urn:ngsi-ld:A:1".parse().expect("valid URI"), "urn:ngsi-ld:A:2".parse().expect("valid URI")];
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let listing = build_listing_query(&params(&snapshot_id, None, Some(&ids)));
        assert!(
            listing.data.to_string(PostgresQueryBuilder).contains("\"entity_id\" = ANY("),
            "{}",
            listing.data.to_string(PostgresQueryBuilder)
        );
    }

    #[test]
    fn an_empty_id_list_adds_no_predicate() {
        let ids: Vec<Uri> = Vec::new();
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let listing = build_listing_query(&params(&snapshot_id, None, Some(&ids)));
        assert!(
            !listing.data.to_string(PostgresQueryBuilder).contains("ANY"),
            "{}",
            listing.data.to_string(PostgresQueryBuilder)
        );
    }

    #[test]
    fn a_single_row_fetch_keys_on_snapshot_and_entity() {
        let snapshot_id: Uri = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot id");
        let entity_id: Uri = "urn:ngsi-ld:A:1".parse().expect("valid entity id");
        let sql = build_get_row_query(Alias::new("snapshot_entities"), Alias::new("entity_data"), &snapshot_id, &entity_id).to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"entity_data\" FROM \"snapshot_entities\""), "{sql}");
        assert!(
            sql.contains("\"snapshot_key\" = (SELECT \"snapshots\".\"snapshot_key\" FROM \"snapshots\" WHERE \"snapshots\".\"id\" = "),
            "{sql}"
        );
        assert!(sql.contains("\"entity_id\" = "), "{sql}");
    }
}
