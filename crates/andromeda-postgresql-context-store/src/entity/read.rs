//! The statements that read Entities' current state by identifier.
//!
//! An Entity is its head (its `entities` and `entity_search` rows) and its
//! `current_attributes` rows. A read by identifier takes them in one
//! statement: the rows are left-joined to the head, so an Entity without
//! attributes, or without one the read selects, still reads back, and nothing
//! is sorted, since assembly orders the rows by ordinal. A retrieval narrowed
//! by `attrs` or a clause 4.21 projection reads the rows of the Attributes it
//! can return and no other. The compare-and-set token a read-modify-write
//! states its precondition against is the head's `modified_at`.
//!
//! A read by identifier is a read request's (a retrieval, or the Entities a
//! query page reads by identifier) or a write's (the state a mutation starts
//! from), and runs on the [`PoolLane`] its caller names.

use crate::{
    current_attribute::read::{RowSelection, Rows, select_row_columns},
    entity::{entity_types::Search, mapper::stored_uri, repo::EntityRepository, stored_entities::GatheredEntities},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, port::retrieved_entity::RetrievedEntity};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_context_query::entity_head::select_head;
use andromeda_postgresql_pool::read_share::PoolLane;
use andromeda_postgresql_schema::current::{CurrentAttributes, E, Entities, EntitySearch};
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use sea_query::{Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgRow};
use std::{collections::HashMap, sync::Arc};

impl EntityRepository {
    /// Retrieve an entity's stored state by identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read or a name lookup fails, or
    /// when the stored rows no longer read back as an entity state.
    pub async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<EntityState>, ContextStoreError> {
        Ok(self
            .get_with_sys_attrs(PoolLane::Whole, target, id, &ReturnedAttributes::Every)
            .await?
            .map(|retrieved| Arc::unwrap_or_clone(retrieved.state)))
    }

    /// Fetch the stored states of several entities, each with its clause 4.8
    /// system attributes, in one statement on `lane`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read or a name lookup fails, or
    /// when the stored rows no longer read back as entity states.
    pub async fn get_batch(
        &self,
        lane: PoolLane,
        target: &ContextTarget,
        ids: &[Identifier],
    ) -> Result<HashMap<Identifier, RetrievedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let id_strs: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
        let retrieved = self
            .read_joined(lane, entities_query(target, any(text_array(&id_strs)), &RowSelection::Every))
            .await?;
        // clone: the identifier keys the map and stays in the state it names
        Ok(retrieved.into_iter().map(|entity| (entity.state.id().clone(), entity)).collect())
    }

    /// Retrieve an entity's stored state together with its `modified_at`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read or a name lookup fails, or
    /// when the stored rows no longer read back as an entity state.
    pub async fn get_snapshot(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, ContextStoreError> {
        Ok(self
            .get_with_sys_attrs(PoolLane::Whole, target, id, &ReturnedAttributes::Every)
            .await?
            .map(snapshot_of))
    }

    /// Retrieve stored states and `modified_at` for several entities in one
    /// statement.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read or a name lookup fails, or
    /// when the stored rows no longer read back as entity states.
    pub async fn get_snapshot_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, ContextStoreError> {
        Ok(self
            .get_batch(PoolLane::Whole, target, ids)
            .await?
            .into_iter()
            .map(|(id, retrieved)| (id, snapshot_of(retrieved)))
            .collect())
    }

    /// Retrieve an entity's stored state together with its clause 4.8 system
    /// attributes, in one statement on `lane`, holding the Attributes
    /// `returned` lets a response hold and no other.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the read or a name lookup fails, or
    /// when the stored rows no longer read back as an entity state.
    pub async fn get_with_sys_attrs(
        &self,
        lane: PoolLane,
        target: &ContextTarget,
        id: &Identifier,
        returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, ContextStoreError> {
        let selection = self.row_selection(returned).await?;
        Ok(self.read_joined(lane, entities_query(target, Expr::val(id.as_str()), &selection)).await?.pop())
    }

    /// The rows holding the Attributes `returned` lets a response hold, its
    /// names resolved through the attribute dictionary. A name that is no
    /// Attribute name is held by no row.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the name lookup fails.
    async fn row_selection(&self, returned: &ReturnedAttributes<'_>) -> Result<RowSelection, ContextStoreError> {
        let iris = match returned {
            ReturnedAttributes::Every => return Ok(RowSelection::Every),
            ReturnedAttributes::Named(iris) | ReturnedAttributes::AllBut(iris) => iris,
        };
        let names: Vec<AttributeName> = iris.iter().filter_map(|iri| iri.as_str().parse().ok()).collect();
        let attributes = self.attribute_names.lookup(&names).await?;
        Ok(RowSelection::of(returned, &attributes))
    }

    /// Retrieve system attributes for a single entity.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the system-attribute query fails.
    pub async fn get_sys_attrs(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<SysAttrsSnapshot>, ContextStoreError> {
        let (sql, values) = sys_attrs_query(target, Expr::val(id.as_str())).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(&self.pool).await?;
        row.as_ref().map(sys_attrs_of).transpose()
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
        let (sql, values) = sys_attrs_query(target, any(text_array(&id_strs))).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| -> Result<(Identifier, SysAttrsSnapshot), ContextStoreError> { Ok((stored_uri(row.try_get("id")?)?, sys_attrs_of(row)?)) })
            .collect()
    }

    /// Run a statement built by [`entities_query`] on `lane` and assemble the
    /// Entities it read.
    async fn read_joined(&self, lane: PoolLane, statement: SelectStatement) -> Result<Vec<RetrievedEntity>, ContextStoreError> {
        let rows = self.fetch_on(lane, &statement).await?;
        self.assembled(GatheredEntities::of_joined(&rows)?).await
    }
}

/// The snapshot a read-modify-write states its precondition against.
fn snapshot_of(retrieved: RetrievedEntity) -> EntitySnapshot {
    EntitySnapshot {
        state: retrieved.state,
        modified_at: retrieved.sys_attrs.modified_at,
    }
}

/// Read the `created_at`, `modified_at` and `expires_at` columns of one row as
/// the entity's clause 4.8 system attributes.
fn sys_attrs_of(row: &PgRow) -> Result<SysAttrsSnapshot, ContextStoreError> {
    Ok(SysAttrsSnapshot {
        created_at: row.try_get("created_at")?,
        modified_at: row.try_get("modified_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

/// The heads of the Entities whose identifier equals `ids`, a bound
/// identifier or `ANY` of a bound array, each with the rows `selection` takes.
/// The selection is a condition of the join, so a head whose rows it takes
/// none of still reads back:
///
/// ```sql
/// SELECT <head>, <row columns>
/// FROM entities e JOIN entity_search s ON s.entity_key = e.entity_key
/// LEFT JOIN current_attributes ca ON ca.entity_key = e.entity_key
///     [AND ca.attribute_name_key = ANY(<keys>) | <> ALL(<keys>)]
/// WHERE e.id = <ids>
/// ```
fn entities_query(target: &ContextTarget, ids: SimpleExpr, selection: &RowSelection) -> SelectStatement {
    let same_entity = Expr::col((Rows, CurrentAttributes::EntityKey)).equals((E, Entities::EntityKey));
    let join = match selection.condition() {
        Some(condition) => same_entity.and(condition),
        None => same_entity,
    };
    let mut statement = Query::select();
    select_head(&mut statement, E, Search);
    select_row_columns(&mut statement);
    statement
        .from_as(table_of(target, Entities::Table), E)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, EntitySearch::Table),
            Search,
            Expr::col((Search, EntitySearch::EntityKey)).equals((E, Entities::EntityKey)),
        )
        .join_as(JoinType::LeftJoin, table_of(target, CurrentAttributes::Table), Rows, join)
        .and_where(Expr::col((E, Entities::Id)).eq(ids));
    statement
}

/// The identifier and system attributes of the Entities whose identifier
/// equals `ids`, read from the identity row alone.
fn sys_attrs_query(target: &ContextTarget, ids: SimpleExpr) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::CreatedAt, Entities::ModifiedAt, Entities::ExpiresAt].map(|column| (E, column)))
        .from_as(table_of(target, Entities::Table), E)
        .and_where(Expr::col((E, Entities::Id)).eq(ids))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute::read::RowSelection,
        entity::read::{entities_query, sys_attrs_query},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use andromeda_postgresql_seaquery::pg_array::{any, text_array};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Value};

    const JOIN: &str = r#"LEFT JOIN "current_attributes" AS "ca" ON "ca"."entity_key" = "e"."entity_key""#;

    fn retrieve() -> String {
        entities_query(&ContextTarget::Live, Expr::val("urn:x"), &RowSelection::Every).to_string(PostgresQueryBuilder)
    }

    fn names(keys: &[i32]) -> Vec<AttributeNameKey> {
        keys.iter().copied().map(AttributeNameKey::from).collect()
    }

    /// Assembly orders the rows by ordinal, so the retrieve reads the
    /// Entity's rows by its key and sorts nothing; an Entity without rows is
    /// kept by the left join.
    #[test]
    fn a_retrieve_reads_rows_without_a_sort() {
        let sql = retrieve();
        assert!(sql.ends_with(&format!(r#"{JOIN} WHERE "e"."id" = 'urn:x'"#)), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    /// Clause 4.21: `attrs=a,b` returns `a` and `b` alone, so only their rows
    /// are joined. The names are a condition of the join, not of the
    /// statement, so the head reads back even when the Entity holds neither.
    #[test]
    fn a_retrieve_narrowed_to_named_attributes_joins_their_rows_alone() {
        let selection = RowSelection::Named(names(&[4, 7]));
        let (sql, values) = entities_query(&ContextTarget::Live, Expr::val("urn:x"), &selection).build(PostgresQueryBuilder);
        assert!(
            sql.ends_with(&format!(r#"{JOIN} AND "ca"."attribute_name_key" = ANY($1) WHERE "e"."id" = $2"#)),
            "{sql}"
        );
        assert_eq!(values.0, vec![Value::from(vec![4_i32, 7]), Value::from("urn:x")]);
    }

    /// Clause 4.21: `pick=id,type` returns no attribute; the head is still
    /// read, with no row joined to it.
    #[test]
    fn a_retrieve_selecting_no_attribute_keeps_the_head() {
        let (sql, values) = entities_query(&ContextTarget::Live, Expr::val("urn:x"), &RowSelection::Named(Vec::new())).build(PostgresQueryBuilder);
        assert!(
            sql.ends_with(&format!(r#"{JOIN} AND "ca"."attribute_name_key" = ANY($1) WHERE "e"."id" = $2"#)),
            "{sql}"
        );
        assert_eq!(values.0[0], Value::from(Vec::<i32>::new()));
    }

    /// Clause 4.21: `omit=a` returns every attribute but `a`.
    #[test]
    fn a_retrieve_omitting_attributes_joins_every_other_row() {
        let (sql, values) = entities_query(&ContextTarget::Live, Expr::val("urn:x"), &RowSelection::AllBut(names(&[4]))).build(PostgresQueryBuilder);
        assert!(
            sql.ends_with(&format!(r#"{JOIN} AND "ca"."attribute_name_key" <> ALL($1) WHERE "e"."id" = $2"#)),
            "{sql}"
        );
        assert_eq!(values.0[0], Value::from(vec![4_i32]));
    }

    /// An omission naming no stored attribute is no selection at all.
    #[test]
    fn a_retrieve_omitting_nothing_joins_every_row() {
        assert_eq!(
            entities_query(&ContextTarget::Live, Expr::val("urn:x"), &RowSelection::AllBut(Vec::new())).to_string(PostgresQueryBuilder),
            retrieve()
        );
    }

    /// The token a read-modify-write states its precondition against is the
    /// Entity's own `modified_at`, read beside the state in one statement.
    #[test]
    fn a_snapshot_read_takes_the_token_from_entities() {
        let sql = retrieve();
        assert!(sql.contains(r#""e"."modified_at" AS "entity_modified_at""#), "{sql}");
        assert!(sql.contains(r#""ca"."modified_at""#), "{sql}");
    }

    #[test]
    fn a_batch_reads_through_one_array_membership_test() {
        let (sql, values) = entities_query(&ContextTarget::Live, any(text_array(&["urn:a", "urn:b"])), &RowSelection::Every).build(PostgresQueryBuilder);
        assert!(
            sql.ends_with(r#"WHERE "e"."id" = ANY($1)"#),
            "a batch must not expand to a placeholder per id: {sql}"
        );
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn system_attributes_read_the_three_lifecycle_instants_from_the_identity_row() {
        assert_eq!(
            sys_attrs_query(&ContextTarget::Live, Expr::val("urn:x")).to_string(PostgresQueryBuilder),
            r#"SELECT "e"."id", "e"."created_at", "e"."modified_at", "e"."expires_at" FROM "entities" AS "e" WHERE "e"."id" = 'urn:x'"#
        );
    }
}
