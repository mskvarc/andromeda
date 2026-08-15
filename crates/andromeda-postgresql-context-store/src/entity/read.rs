//! The statements that read the `entities` table.

use crate::{
    entity::{mapper::stored_uri, pushdown, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_entity::{entity_snapshot::EntitySnapshot, query_outcome::QueryOutcome};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_postgresql_context_query::entity as entity_sql;
use andromeda_postgresql_query::{projected_attribute_names::projected_attribute_names, q_predicate::attribute_names::ResolvedAttributeNames};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::collections::HashMap;

impl EntityRepository {
    /// Retrieve an entity's expanded payload by identifier.
    pub async fn get(&self, id: &Uri) -> Result<Option<Value>, ContextStoreError> {
        let (sql, values) = payload_by_id_query(id.as_str()).build_sqlx(PostgresQueryBuilder);
        let result: Option<Value> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    /// Fetch expanded payloads for multiple entities in one query.
    pub async fn get_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, Value>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Uri::as_str).collect();
        let (sql, values) = payloads_by_ids_query(&id_strs).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        let mut result = HashMap::with_capacity(ids.len());
        for row in rows {
            let id = stored_uri(row.get("id"))?;
            let payload: Value = row.get("payload");
            result.insert(id, payload);
        }
        Ok(result)
    }

    /// Retrieve an entity's payload together with its `modified_at`.
    pub async fn get_snapshot(&self, id: &Uri) -> Result<Option<EntitySnapshot>, ContextStoreError> {
        let (sql, values) = snapshot_by_id_query(id.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;

        Ok(row.map(|row| EntitySnapshot {
            payload: row.get("payload"),
            modified_at: row.get("modified_at"),
        }))
    }

    /// Retrieve payloads and `modified_at` for several entities in one query.
    pub async fn get_snapshot_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Uri::as_str).collect();
        let (sql, values) = snapshots_by_ids_query(&id_strs).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        let mut result = HashMap::with_capacity(ids.len());
        for row in rows {
            let id = stored_uri(row.get("id"))?;
            result.insert(
                id,
                EntitySnapshot {
                    payload: row.get("payload"),
                    modified_at: row.get("modified_at"),
                },
            );
        }
        Ok(result)
    }

    /// Retrieve system attributes for a single entity.
    pub async fn get_sys_attrs(&self, id: &Uri) -> Result<Option<SysAttrsSnapshot>, ContextStoreError> {
        let (sql, values) = sys_attrs_by_id_query(id.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;

        Ok(row.map(|row| SysAttrsSnapshot {
            created_at: row.get("created_at"),
            modified_at: row.get("modified_at"),
            expires_at: row.get("expires_at"),
        }))
    }

    /// Fetch system attributes for multiple entities in one query.
    pub async fn get_sys_attrs_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Uri::as_str).collect();
        let (sql, values) = sys_attrs_by_ids_query(&id_strs).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;

        let mut result = HashMap::with_capacity(rows.len());
        for row in rows {
            let id = stored_uri(row.get("id"))?;
            result.insert(
                id,
                SysAttrsSnapshot {
                    created_at: row.get("created_at"),
                    modified_at: row.get("modified_at"),
                    expires_at: row.get("expires_at"),
                },
            );
        }
        Ok(result)
    }

    /// Execute a compiled entity query and return matching payloads.
    ///
    /// What is sent and what is handed back are decided by this store's
    /// [`StorageProfile`](andromeda_entity::storage_profile::StorageProfile).
    /// With PostGIS and `ltree` every predicate is sent and nothing is handed
    /// back, so the residual is always empty here -- but the decision is read
    /// off the profile rather than assumed, which is what a store without
    /// those extensions changes.
    pub async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, ContextStoreError> {
        let planned = pushdown::plan(compiled, &EntityRepository::PROFILE);
        let attribute_names = self.resolve_projected_attributes(&planned.query).await?;
        let built = entity_sql::build_query(&planned.query, &attribute_names)?;

        let has_count = built.count.is_some();
        let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
        let result_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let (rows, window_count) = extract_payloads(result_rows, has_count)?;

        let total_count = match (built.count, window_count, rows.is_empty()) {
            (Some(_), Some(count), _) => Some(count),
            (Some(count), None, true) => {
                let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
                Some(
                    sqlx::query_scalar_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
                        .fetch_one(&self.pool)
                        .await?,
                )
            }
            (Some(_), None, false) | (None, _, _) => None,
        };

        Ok(planned.outcome(rows, total_count))
    }

    /// Execute a compiled query and return only the matching entity identifiers.
    ///
    /// Planned the same way [`EntityRepository::query_compiled`] is; the
    /// statement never counts, so the outcome carries no total either way.
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, ContextStoreError> {
        let planned = pushdown::plan(compiled, &EntityRepository::PROFILE);
        let attribute_names = self.resolve_projected_attributes(&planned.query).await?;
        let statement = entity_sql::build_ids_query(&planned.query, &attribute_names)?;

        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let ids: Vec<Uri> = rows.iter().map(|row| stored_uri(row.get("id"))).collect::<Result<_, _>>()?;
        Ok(planned.outcome(ids, None))
    }

    /// Resolve the attribute IRIs a compiled query reads by dictionary key.
    ///
    /// A clause 4.9 linked-entity traversal contributes more than one name per
    /// predicate: the Relationship each hop descends through is probed by
    /// dictionary key just as the terminal attribute is, and both come back from
    /// the same collection walk. A geo-query and a distance ordering each
    /// contribute their target GeoProperty, which is read from the same
    /// projection (clauses 4.10 and 4.23).
    ///
    /// The lookup needs no barrier in front of it. Dictionary rows and the
    /// projected values keyed on them are written in the entity's own
    /// transaction, so a name that resolves to nothing is a name nothing has
    /// ever written, not one written moments ago.
    async fn resolve_projected_attributes(&self, compiled: &CompiledEntityQuery) -> Result<ResolvedAttributeNames, ContextStoreError> {
        let names = projected_attribute_names(compiled.selection.q_expr.as_ref(), compiled.selection.geo.as_ref(), &compiled.ordering.terms);
        if names.is_empty() {
            return Ok(ResolvedAttributeNames::default());
        }
        Ok(self.attribute_names.lookup(&names).await?)
    }
}

/// Extract entity payloads from a result set, optionally reading the piggybacked
/// total count off the first row (`COUNT(*) OVER ()` is constant across the set).
fn extract_payloads(rows: Vec<PgRow>, read_total_count: bool) -> Result<(Vec<Value>, Option<i64>), ContextStoreError> {
    let mut row_iter = rows.into_iter();
    if read_total_count {
        if let Some(first) = row_iter.next() {
            let count = first.try_get::<i64, _>(entity_sql::TOTAL_COUNT_COLUMN).ok();
            let mut results = Vec::with_capacity(row_iter.size_hint().0 + 1);
            results.push(first.try_get::<Value, _>("payload")?);
            for row in row_iter {
                results.push(row.try_get::<Value, _>("payload")?);
            }
            return Ok((results, count));
        }
        return Ok((Vec::new(), None));
    }
    let results: Vec<Value> = row_iter.map(|row| row.try_get::<Value, _>("payload")).collect::<Result<_, _>>()?;
    Ok((results, None))
}

/// The payload of the entity identified by `id`.
fn payload_by_id_query(id: &str) -> SelectStatement {
    Query::select()
        .column(Entities::Payload)
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(id))
        .to_owned()
}

/// The identifier and payload of every entity in `ids`.
fn payloads_by_ids_query(ids: &[&str]) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::Payload])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(any(text_array(ids))))
        .to_owned()
}

/// The payload and version token of the entity identified by `id`.
fn snapshot_by_id_query(id: &str) -> SelectStatement {
    Query::select()
        .columns([Entities::Payload, Entities::ModifiedAt])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(id))
        .to_owned()
}

/// The identifier, payload and version token of every entity in `ids`.
fn snapshots_by_ids_query(ids: &[&str]) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::Payload, Entities::ModifiedAt])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(any(text_array(ids))))
        .to_owned()
}

/// The system attributes of the entity identified by `id`.
fn sys_attrs_by_id_query(id: &str) -> SelectStatement {
    Query::select()
        .columns([Entities::CreatedAt, Entities::ModifiedAt, Entities::ExpiresAt])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(id))
        .to_owned()
}

/// The identifier and system attributes of every entity in `ids`.
fn sys_attrs_by_ids_query(ids: &[&str]) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::CreatedAt, Entities::ModifiedAt, Entities::ExpiresAt])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(any(text_array(ids))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::entity::read::{payload_by_id_query, payloads_by_ids_query, snapshot_by_id_query, sys_attrs_by_id_query, sys_attrs_by_ids_query};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_single_payload_reads_by_id() {
        assert_eq!(
            payload_by_id_query("urn:x").to_string(PostgresQueryBuilder),
            "SELECT \"payload\" FROM \"entities\" WHERE \"id\" = 'urn:x'"
        );
    }

    #[test]
    fn a_batch_reads_through_one_array_membership_test() {
        let sql = payloads_by_ids_query(&["urn:a", "urn:b"]).to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"id\", \"payload\" FROM \"entities\""), "{sql}");
        assert!(sql.contains("\"id\" = ANY("), "a batch must not expand to a placeholder per id: {sql}");
    }

    #[test]
    fn a_snapshot_reads_the_payload_with_its_version_token() {
        let sql = snapshot_by_id_query("urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"payload\"") && sql.contains("\"modified_at\""), "{sql}");
    }

    #[test]
    fn system_attributes_read_the_three_lifecycle_instants() {
        let single = sys_attrs_by_id_query("urn:x").to_string(PostgresQueryBuilder);
        let batch = sys_attrs_by_ids_query(&["urn:a"]).to_string(PostgresQueryBuilder);
        for column in ["\"created_at\"", "\"modified_at\"", "\"expires_at\""] {
            assert!(single.contains(column), "{single}");
            assert!(batch.contains(column), "{batch}");
        }
    }
}
