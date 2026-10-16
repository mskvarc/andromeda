//! Reading whole Entities in bulk, by type or across the whole space.
//!
//! Neither answer aggregates: both read the head of every Entity the caller
//! asked for, ordered by entity id so an offset names a stable window, and then
//! the rows of the Entities the window kept.

use crate::{
    current_attribute::read::RowSelection,
    entity::{entity_types::Search, repo::EntityRepository, stored_entities::heads_statement},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_pool::read_share::PoolLane;
use andromeda_postgresql_schema::current::{E, Entities, EntitySearch};
use andromeda_postgresql_seaquery::operator::array_contains_key;
use sea_query::{Expr, Order, PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use std::{slice::from_ref, sync::Arc};

/// The unsigned page bound a signed `limit` or `offset` names.
///
/// A negative value is not a narrower bound but an absent one, so it clamps to
/// zero rather than wrapping into an enormous unsigned width.
fn page_bound(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

impl EntityRepository {
    /// Retrieve the stored states of the entities of one type, with optional
    /// pagination.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the dictionary lookup for the type, a
    /// read or a name lookup fails, or when the stored rows no longer read back
    /// as entity states.
    pub async fn get_entities_by_type(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<EntityState>, ContextStoreError> {
        // A type the dictionary never interned was never carried by any entity.
        let Some(type_key) = self.entity_type_names.lookup(from_ref(entity_type)).await?.get(entity_type) else {
            return Ok(Vec::new());
        };
        let mut statement = heads_statement(target);
        statement.and_where(array_contains_key(Expr::col((Search, EntitySearch::EntityTypeNameKeys)), type_key.get()));
        self.stored_states(target, windowed(statement, limit, offset)).await
    }

    /// Retrieve the stored states of every entity, with optional pagination.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a read or a name lookup fails, or
    /// when the stored rows no longer read back as entity states.
    pub async fn get_all_entities(&self, target: &ContextTarget, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<EntityState>, ContextStoreError> {
        self.stored_states(target, windowed(heads_statement(target), limit, offset)).await
    }

    /// Run a statement selecting heads and read the states of the Entities it
    /// selected, in its order.
    async fn stored_states(&self, target: &ContextTarget, statement: SelectStatement) -> Result<Vec<EntityState>, ContextStoreError> {
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let heads = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        Ok(self
            .entities_of_heads(PoolLane::Whole, target, &heads, &RowSelection::Every)
            .await?
            .into_iter()
            .map(|retrieved| Arc::unwrap_or_clone(retrieved.state))
            .collect())
    }
}

/// `statement` ordered by entity id, from `offset` and at most `limit` long.
fn windowed(mut statement: SelectStatement, limit: Option<i64>, offset: Option<i64>) -> SelectStatement {
    statement.order_by((E, Entities::Id), Order::Asc).offset(page_bound(offset.unwrap_or(0)));
    if let Some(limit) = limit {
        statement.limit(page_bound(limit));
    }
    statement
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        state_listing::{page_bound, windowed},
        stored_entities::heads_statement,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// A negative bound names no page at all, and clamping rather than wrapping
    /// is what keeps it from becoming an enormous unsigned one.
    #[test]
    fn a_negative_page_bound_clamps_to_zero() {
        assert_eq!(page_bound(-1), 0);
        assert_eq!(page_bound(i64::MIN), 0);
    }

    #[test]
    fn a_non_negative_page_bound_is_carried_through() {
        assert_eq!(page_bound(0), 0);
        assert_eq!(page_bound(25), 25);
        assert_eq!(page_bound(i64::MAX), 9_223_372_036_854_775_807);
    }

    /// The window is cut over heads alone, in identifier order; the rows of
    /// the kept Entities are read afterwards by key.
    #[test]
    fn a_listing_windows_the_heads_in_identifier_order() {
        let sql = windowed(heads_statement(&ContextTarget::Live), Some(10), Some(20)).to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"ORDER BY "e"."id" ASC LIMIT 10 OFFSET 20"#), "{sql}");
        assert!(!sql.contains("current_attributes"), "{sql}");
    }
}
