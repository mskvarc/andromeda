//! The statements that list the entities a materialised snapshot holds
//! (clause 5.16, Snapshot Functionality).
//!
//! A sibling of [`snapshot_purge`](crate::snapshot_purge): both assemble a
//! snapshot statement in the [`BindBuilder`]/[`BuiltQuery`] style rather than
//! through sea-query, so the whole snapshot statement family reads one way. The
//! runtime table and column names come from the caller because the same shape
//! serves both the entity snapshot table and the temporal snapshot table; they
//! are trusted identifiers the repository supplies, never request input.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_seaquery::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
};
use sea_query::Iden;
use std::borrow::Cow;

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

/// Build the count-and-data statement pair that lists a snapshot's entities.
///
/// The returned [`BuiltQuery`] carries the paginated data statement in `sql`,
/// the matching `COUNT(*)` in `count_sql`, and the shared `WHERE`-clause binds.
/// `LIMIT`/`OFFSET` are rendered as integer literals rather than binds so the
/// count statement, which omits them, reuses the same bind vector — the entity
/// query builder pages the same way.
pub fn build_listing_query(params: &SnapshotListingParams<'_>) -> BuiltQuery<'static> {
    let mut binds: BindBuilder<BindValue<'static>> = BindBuilder::default();

    let snapshot_marker = binds.bind(BindValue::String(Cow::Owned(params.snapshot_id.to_string())));
    let mut predicates = vec![format!("snapshot_id = {snapshot_marker}")];

    if let Some(entity_type) = params.entity_type {
        let marker = binds.bind(BindValue::String(Cow::Owned(entity_type.to_string())));
        predicates.push(format!("{}->>'type' LIKE '%' || {marker} || '%'", params.data_column));
    }

    if let Some(ids) = params.entity_ids
        && !ids.is_empty()
    {
        let ids = ids.iter().map(Uri::to_string).collect();
        let marker = binds.bind(BindValue::StringVec(Cow::Owned(ids)));
        predicates.push(format!("entity_id = ANY({marker})"));
    }

    let where_clause = predicates.join(" AND ");
    let table = params.table;
    let data_column = params.data_column;
    // `LIMIT`/`OFFSET` are non-negative integers, so they are safe to inline and
    // must be inlined for the count statement to share this bind vector.
    let limit = params.limit.max(0);
    let offset = params.offset.max(0);

    BuiltQuery {
        sql: format!("SELECT {data_column} FROM {table} WHERE {where_clause} ORDER BY entity_id ASC LIMIT {limit} OFFSET {offset}"),
        count_sql: Some(format!("SELECT COUNT(*) AS cnt FROM {table} WHERE {where_clause}")),
        bind_values: binds.into_values(),
    }
}

/// Build the statement that fetches one entity's representation from a snapshot.
///
/// `table` and `data_column` are the SQL identifiers the repository supplies,
/// named through [`Iden`] so a rename moves the reference with them; the snapshot
/// and the entity are named by their domain identifiers.
pub fn build_get_row_query(table: impl Iden, data_column: impl Iden, snapshot_id: &Uri, entity_id: &Uri) -> BuiltQuery<'static> {
    let mut binds: BindBuilder<BindValue<'static>> = BindBuilder::default();
    let snapshot_marker = binds.bind(BindValue::String(Cow::Owned(snapshot_id.to_string())));
    let entity_marker = binds.bind(BindValue::String(Cow::Owned(entity_id.to_string())));

    let table = table.to_string();
    let data_column = data_column.to_string();
    BuiltQuery {
        sql: format!("SELECT {data_column} FROM {table} WHERE snapshot_id = {snapshot_marker} AND entity_id = {entity_marker}"),
        count_sql: None,
        bind_values: binds.into_values(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::Alias;

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
    fn the_bare_listing_filters_only_by_snapshot_and_pages_by_literal() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let built = build_listing_query(&params(&snapshot_id, None, None));
        assert_eq!(
            built.sql,
            "SELECT entity_data FROM snapshot_entities WHERE snapshot_id = $1 ORDER BY entity_id ASC LIMIT 25 OFFSET 50"
        );
        assert_eq!(
            built.count_sql.as_deref(),
            Some("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1")
        );
        assert_eq!(built.bind_values.len(), 1);
    }

    #[test]
    fn a_type_filter_matches_the_json_type_member() {
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let entity_type = "Vehicle".parse().expect("valid type expression");
        let built = build_listing_query(&params(&snapshot_id, Some(&entity_type), None));
        assert!(built.sql.contains("entity_data->>'type' LIKE '%' || $2 || '%'"), "{}", built.sql);
        // The count shares the type predicate and its bind, but not the paging.
        assert!(
            built
                .count_sql
                .as_deref()
                .unwrap_or_default()
                .contains("entity_data->>'type' LIKE '%' || $2 || '%'")
        );
        assert!(!built.count_sql.as_deref().unwrap_or_default().contains("LIMIT"));
        assert_eq!(built.bind_values.len(), 2);
    }

    #[test]
    fn an_id_filter_renders_an_array_membership_test() {
        let ids = vec!["urn:ngsi-ld:A:1".parse().expect("valid URI"), "urn:ngsi-ld:A:2".parse().expect("valid URI")];
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let built = build_listing_query(&params(&snapshot_id, None, Some(&ids)));
        assert!(built.sql.contains("entity_id = ANY($2)"), "{}", built.sql);
        assert_eq!(built.bind_values.len(), 2);
    }

    #[test]
    fn an_empty_id_list_adds_no_predicate() {
        let ids: Vec<Uri> = Vec::new();
        let snapshot_id = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot URI");
        let built = build_listing_query(&params(&snapshot_id, None, Some(&ids)));
        assert!(!built.sql.contains("ANY"), "{}", built.sql);
        assert_eq!(built.bind_values.len(), 1);
    }

    #[test]
    fn a_single_row_fetch_keys_on_snapshot_and_entity() {
        let snapshot_id: Uri = "urn:ngsi-ld:Snapshot:1".parse().expect("valid snapshot id");
        let entity_id: Uri = "urn:ngsi-ld:A:1".parse().expect("valid entity id");
        let built = build_get_row_query(Alias::new("snapshot_entities"), Alias::new("entity_data"), &snapshot_id, &entity_id);
        assert_eq!(built.sql, "SELECT entity_data FROM snapshot_entities WHERE snapshot_id = $1 AND entity_id = $2");
        assert!(built.count_sql.is_none());
        assert_eq!(built.bind_values.len(), 2);
    }
}
