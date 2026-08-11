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
use andromeda_postgresql_query::{bind::BindValue, projected_attribute_names::projected_attribute_names, q_predicate::attribute_names::ResolvedAttributeNames};
use serde_json::Value;
use sqlx::Row;
use std::collections::HashMap;

impl EntityRepository {
    /// Retrieve an entity's expanded payload by identifier.
    pub async fn get(&self, id: &Uri) -> Result<Option<Value>, ContextStoreError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id.as_str())
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
        let rows = sqlx::query("SELECT id, payload FROM entities WHERE id = ANY($1)")
            .bind(&id_strs)
            .fetch_all(&self.pool)
            .await?;

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
        let row = sqlx::query("SELECT payload, modified_at FROM entities WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;

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
        let rows = sqlx::query("SELECT id, payload, modified_at FROM entities WHERE id = ANY($1)")
            .bind(&id_strs)
            .fetch_all(&self.pool)
            .await?;

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
        let row = sqlx::query("SELECT created_at, modified_at, expires_at FROM entities WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;

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
        let rows = sqlx::query("SELECT id, created_at, modified_at, expires_at FROM entities WHERE id = ANY($1)")
            .bind(&id_strs)
            .fetch_all(&self.pool)
            .await?;

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

        tracing::debug!("Built SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let (rows, window_count) = self.execute_dynamic_query(&built.sql, &built.bind_values, built.count_sql.is_some()).await?;

        let total_count = match (&built.count_sql, window_count, rows.is_empty()) {
            (Some(_), Some(count), _) => Some(count),
            (Some(count_sql), None, true) => Some(self.execute_count_query(count_sql, &built.bind_values).await?),
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
        let built = entity_sql::build_ids_query(&planned.query, &attribute_names)?;

        tracing::debug!("Built IDs SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let query = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values);

        let rows = query.fetch_all(&self.pool).await?;
        let ids: Vec<Uri> = rows.iter().map(|row| stored_uri(row.get("id"))).collect::<Result<_, _>>()?;
        Ok(planned.outcome(ids, None))
    }

    /// Resolve the attribute IRIs a compiled query reads by dictionary key, and
    /// wait for the `attribute_current` projection to catch up if any of them
    /// will be answered from it.
    ///
    /// A clause 4.9 linked-entity traversal contributes more than one name per
    /// predicate: the Relationship each hop descends through is probed by
    /// dictionary key just as the terminal attribute is, and both come back from
    /// the same collection walk. A geo-query and a distance ordering each
    /// contribute their target GeoProperty, which is read from the same
    /// projection (clauses 4.10 and 4.23).
    async fn resolve_projected_attributes(&self, compiled: &CompiledEntityQuery) -> Result<ResolvedAttributeNames, ContextStoreError> {
        let names = projected_attribute_names(compiled.selection.q_expr.as_ref(), compiled.selection.geo.as_ref(), &compiled.ordering.terms);
        if names.is_empty() {
            return Ok(ResolvedAttributeNames::default());
        }
        // The barrier is crossed before the dictionary is read, not after it
        // resolved something: `attribute_names` rows are minted by the same
        // projection worker, so a name looked up first would read as unknown
        // for an attribute written moments ago and compile to constant `FALSE`
        // (clause 4.10) or to a distance that measures from nothing (clause
        // 4.23) — a read-your-writes violation rather than an empty answer.
        self.attribute_current.wait_for_pending_writes().await;
        Ok(self.attribute_names.lookup(&names).await?)
    }

    /// Run a dynamic query and extract payloads, optionally reading the total count from the first row.
    async fn execute_dynamic_query(&self, sql: &str, binds: &[BindValue<'_>], read_total_count: bool) -> Result<(Vec<Value>, Option<i64>), ContextStoreError> {
        let query = andromeda_postgresql_query::bind_values!(sqlx::query(sql), binds);

        let rows = query.fetch_all(&self.pool).await?;
        let mut row_iter = rows.into_iter();
        let total_count = if read_total_count {
            match row_iter.next() {
                Some(first) => {
                    let count = first.try_get::<i64, _>(entity_sql::TOTAL_COUNT_COLUMN).ok();
                    let data: Value = first.try_get("payload")?;
                    let mut results = Vec::with_capacity(row_iter.size_hint().0 + 1);
                    results.push(data);
                    for row in row_iter {
                        results.push(row.try_get::<Value, _>("payload")?);
                    }
                    return Ok((results, count));
                }
                None => None,
            }
        } else {
            None
        };
        let results: Vec<Value> = row_iter.map(|row| row.try_get::<Value, _>("payload")).collect::<Result<_, _>>()?;
        Ok((results, total_count))
    }

    /// Run a standalone count query.
    async fn execute_count_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<i64, ContextStoreError> {
        let query = andromeda_postgresql_query::bind_values!(sqlx::query_scalar(sql), binds);

        let count: i64 = query.fetch_one(&self.pool).await?;
        Ok(count)
    }
}
