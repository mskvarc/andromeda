//! The statements that read one entity's current state.
//!
//! The payload and its version token live on `entity_documents`, the
//! identifier and lifecycle instants on `entities`; every read here joins the
//! two on the surrogate key and names its columns by relation.

use crate::{
    entity::{mapper::stored_uri, pushdown, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, query_outcome::QueryOutcome};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_context_query::{
    entity::{self as entity_sql, EntityTotal},
    entity_page::TOTAL_COUNT_COLUMN,
};
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments};
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use iri_rs::IriBuf;
use sea_query::{Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::collections::HashMap;

impl EntityRepository {
    /// Retrieve an entity's expanded payload by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the payload query fails.
    pub async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<Value>, ContextStoreError> {
        let (sql, values) = payload_by_id_query(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let result: Option<Value> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    /// Fetch expanded payloads for multiple entities in one query.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the payload query fails, or when a
    /// returned identifier is no longer a valid URI.
    pub async fn get_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, Value>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
        let (sql, values) = payloads_by_ids_query(target, &id_strs).build_sqlx(PostgresQueryBuilder);
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
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the snapshot query fails.
    pub async fn get_snapshot(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, ContextStoreError> {
        let (sql, values) = snapshot_by_id_query(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;

        Ok(row.map(|row| EntitySnapshot {
            payload: row.get("payload"),
            modified_at: row.get("modified_at"),
        }))
    }

    /// Retrieve payloads and `modified_at` for several entities in one query.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the snapshot query fails, or when a
    /// returned identifier is no longer a valid URI.
    pub async fn get_snapshot_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
        let (sql, values) = snapshots_by_ids_query(target, &id_strs).build_sqlx(PostgresQueryBuilder);
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
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the system-attribute query fails.
    pub async fn get_sys_attrs(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<SysAttrsSnapshot>, ContextStoreError> {
        let (sql, values) = sys_attrs_by_id_query(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;

        Ok(row.map(|row| SysAttrsSnapshot {
            created_at: row.get("created_at"),
            modified_at: row.get("modified_at"),
            expires_at: row.get("expires_at"),
        }))
    }

    /// Fetch system attributes for multiple entities in one query.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the system-attribute query fails, or
    /// when a returned identifier is no longer a valid URI.
    pub async fn get_sys_attrs_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
        let (sql, values) = sys_attrs_by_ids_query(target, &id_strs).build_sqlx(PostgresQueryBuilder);
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
    /// With `PostGIS` and `ltree` every predicate is sent and nothing is handed
    /// back, so the residual is always empty here -- but the decision is read
    /// off the profile rather than assumed, which is what a store without
    /// those extensions changes.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when the select or the separate count fails, or
    /// when a returned row does not carry the expected payload column.
    pub async fn query_compiled(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, ContextStoreError> {
        let planned = pushdown::plan(compiled, EntityRepository::PROFILE);
        self.ensure_collations(&planned.query.ordering).await?;
        let names = self.resolve_names(&planned.query).await?;
        let built = entity_sql::build_query(target, &planned.query, &names)?;

        let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
        let result_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let on_page_rows = matches!(built.total, EntityTotal::OnPageRows { .. });
        let (rows, window_count) = extract_payloads(result_rows, on_page_rows)?;

        let total_count = match built.total {
            EntityTotal::Omitted => None,
            EntityTotal::OnPageRows { empty_page } => match (window_count, rows.is_empty()) {
                (Some(count), _) => Some(count),
                (None, true) => Some(self.count_matches(empty_page).await?),
                (None, false) => None,
            },
            EntityTotal::Counted(count) => Some(self.count_matches(count).await?),
        };

        Ok(planned.outcome(rows, total_count))
    }

    /// Run a `COUNT` statement and read back its one value.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the statement fails.
    async fn count_matches(&self, count: SelectStatement) -> Result<i64, ContextStoreError> {
        let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Execute a compiled query and return only the matching entity identifiers.
    ///
    /// Planned the same way [`EntityRepository::query_compiled`] is; the
    /// statement never counts, so the outcome carries no total either way.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when the select fails, or when a returned
    /// identifier is no longer a valid URI.
    pub async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Identifier>, ContextStoreError> {
        let planned = pushdown::plan(compiled, EntityRepository::PROFILE);
        self.ensure_collations(&planned.query.ordering).await?;
        let names = self.resolve_names(&planned.query).await?;
        let statement = entity_sql::build_ids_query(target, &planned.query, &names)?;

        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let ids: Vec<Identifier> = rows.iter().map(|row| stored_uri(row.get("id"))).collect::<Result<_, _>>()?;
        Ok(planned.outcome(ids, None))
    }

    /// Resolve the attribute and entity type IRIs a compiled query reads by
    /// dictionary key.
    ///
    /// A clause 5.5.9 `attrs` term is answered by the search row's interned
    /// attribute names, so every term resolves, and so is every name a clause
    /// 4.21 projection keeps an entity alive by. A clause 4.9 linked-entity
    /// traversal contributes more than one name per predicate: the Relationship
    /// each hop descends through is probed by dictionary key just as the
    /// terminal attribute is, and both come back from the same collection walk.
    /// A geo-query and a distance ordering each contribute their target
    /// `GeoProperty`, which is read from the same projection (clauses 4.10 and
    /// 4.23). The type selection of every selector
    /// (clause 4.17) and the type guard of every traversal hop are tested
    /// against the interned `entity_type_name_keys` column, so they resolve through the type
    /// dictionary.
    ///
    /// The lookups need no barrier in front of them. Dictionary rows and the
    /// rows keyed on them are written in the entity's own transaction, so a
    /// name that resolves to nothing is a name nothing has ever written, not
    /// one written moments ago.
    async fn resolve_names(&self, compiled: &CompiledEntityQuery) -> Result<QueryNames, ContextStoreError> {
        let members = compiled
            .selection
            .attrs
            .iter()
            .map(IriBuf::as_str)
            .chain(compiled.selection.projected_members.names());
        let attribute_names = projected_attribute_names(
            members,
            compiled.selection.q_expr.as_ref(),
            compiled.selection.geo.as_ref(),
            &compiled.ordering.terms,
        );
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names.lookup(&attribute_names).await?
        };
        let type_names = projected_type_names(&compiled.selection);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names.lookup(&type_names).await?
        };
        Ok(QueryNames { attributes, types })
    }
}

/// Extract entity payloads from a result set, optionally reading the piggybacked
/// total count off the first row (`COUNT(*) OVER ()` is constant across the set).
fn extract_payloads(rows: Vec<PgRow>, read_total_count: bool) -> Result<(Vec<Value>, Option<i64>), ContextStoreError> {
    let mut row_iter = rows.into_iter();
    if read_total_count {
        if let Some(first) = row_iter.next() {
            let count = first.try_get::<i64, _>(TOTAL_COUNT_COLUMN).ok();
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

/// `FROM entities JOIN entity_documents ON entity_documents.entity_key = entities.entity_key`,
/// the source every read of a payload or version token starts from.
pub(super) fn identity_with_documents(target: &ContextTarget) -> SelectStatement {
    Query::select()
        .from(table_of(target, Entities::Table))
        .join(
            JoinType::Join,
            table_of(target, EntityDocuments::Table),
            Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        )
        .take()
}

/// The payload of the entity identified by `id`.
fn payload_by_id_query(target: &ContextTarget, id: &str) -> SelectStatement {
    identity_with_documents(target)
        .column((EntityDocuments::Table, EntityDocuments::Payload))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id))
        .take()
}

/// The identifier and payload of every entity in `ids`.
fn payloads_by_ids_query(target: &ContextTarget, ids: &[&str]) -> SelectStatement {
    identity_with_documents(target)
        .column((Entities::Table, Entities::Id))
        .column((EntityDocuments::Table, EntityDocuments::Payload))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(any(text_array(ids))))
        .take()
}

/// The payload and version token of the entity identified by `id`.
fn snapshot_by_id_query(target: &ContextTarget, id: &str) -> SelectStatement {
    identity_with_documents(target)
        .column((EntityDocuments::Table, EntityDocuments::Payload))
        .column((EntityDocuments::Table, EntityDocuments::ModifiedAt))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id))
        .take()
}

/// The identifier, payload and version token of every entity in `ids`.
fn snapshots_by_ids_query(target: &ContextTarget, ids: &[&str]) -> SelectStatement {
    identity_with_documents(target)
        .column((Entities::Table, Entities::Id))
        .column((EntityDocuments::Table, EntityDocuments::Payload))
        .column((EntityDocuments::Table, EntityDocuments::ModifiedAt))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(any(text_array(ids))))
        .take()
}

/// The system attributes of the entity identified by `id`.
fn sys_attrs_by_id_query(target: &ContextTarget, id: &str) -> SelectStatement {
    identity_with_documents(target)
        .column((Entities::Table, Entities::CreatedAt))
        .column((EntityDocuments::Table, EntityDocuments::ModifiedAt))
        .column((Entities::Table, Entities::ExpiresAt))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id))
        .take()
}

/// The identifier and system attributes of every entity in `ids`.
fn sys_attrs_by_ids_query(target: &ContextTarget, ids: &[&str]) -> SelectStatement {
    identity_with_documents(target)
        .column((Entities::Table, Entities::Id))
        .column((Entities::Table, Entities::CreatedAt))
        .column((EntityDocuments::Table, EntityDocuments::ModifiedAt))
        .column((Entities::Table, Entities::ExpiresAt))
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(any(text_array(ids))))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::entity::read::{payload_by_id_query, payloads_by_ids_query, snapshot_by_id_query, sys_attrs_by_id_query, sys_attrs_by_ids_query};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn target() -> ContextTarget {
        ContextTarget::Live
    }

    const JOIN: &str = r#"FROM "entities" JOIN "entity_documents" ON "entity_documents"."entity_key" = "entities"."entity_key""#;

    #[test]
    fn a_single_payload_reads_by_id_through_the_document_join() {
        assert_eq!(
            payload_by_id_query(&target(), "urn:x").to_string(PostgresQueryBuilder),
            format!(r#"SELECT "entity_documents"."payload" {JOIN} WHERE "entities"."id" = 'urn:x'"#)
        );
    }

    #[test]
    fn a_batch_reads_through_one_array_membership_test() {
        let sql = payloads_by_ids_query(&target(), &["urn:a", "urn:b"]).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(&format!(r#"SELECT "entities"."id", "entity_documents"."payload" {JOIN}"#)),
            "{sql}"
        );
        assert!(
            sql.contains(r#""entities"."id" = ANY("#),
            "a batch must not expand to a placeholder per id: {sql}"
        );
    }

    #[test]
    fn a_snapshot_reads_the_payload_with_its_version_token_from_the_document_row() {
        let sql = snapshot_by_id_query(&target(), "urn:x").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""entity_documents"."payload", "entity_documents"."modified_at""#), "{sql}");
    }

    #[test]
    fn system_attributes_read_the_three_lifecycle_instants_from_their_relations() {
        let single = sys_attrs_by_id_query(&target(), "urn:x").to_string(PostgresQueryBuilder);
        let batch = sys_attrs_by_ids_query(&target(), &["urn:a"]).to_string(PostgresQueryBuilder);
        for column in [
            r#""entities"."created_at""#,
            r#""entity_documents"."modified_at""#,
            r#""entities"."expires_at""#,
        ] {
            assert!(single.contains(column), "{single}");
            assert!(batch.contains(column), "{batch}");
        }
    }
}
