//! Reading whole entity payloads in bulk, by type or across the whole space.
//!
//! Neither answer aggregates: both project the stored document of every entity
//! the caller asked for, ordered by entity id so an offset names a stable
//! window.

use crate::{
    entity::{
        read::{identity_with_documents, payload_of},
        repo::EntityRepository,
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::operator::array_contains_key;
use sea_query::{Expr, ExprTrait, JoinType, Order, PostgresQueryBuilder};
use sea_query_sqlx::{SqlxBinder, SqlxValues};
use std::slice::from_ref;

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
    /// Returns [`ContextStoreError`] when the dictionary lookup for the type or
    /// the payload query fails, or when a stored document no longer decodes as
    /// an entity state.
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
        let mut statement = identity_with_documents(target);
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .join(
                JoinType::Join,
                table_of(target, EntitySearch::Table),
                Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
            )
            .and_where(array_contains_key(
                Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys)),
                type_key.get(),
            ))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(page_bound(offset.unwrap_or(0)));
        if let Some(limit) = limit {
            statement.limit(page_bound(limit));
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        self.stored_states(&sql, values).await
    }

    /// Retrieve the stored states of every entity, with optional pagination.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the payload query fails or a stored
    /// document no longer decodes as an entity state.
    pub async fn get_all_entities(&self, target: &ContextTarget, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<EntityState>, ContextStoreError> {
        let mut statement = identity_with_documents(target);
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(page_bound(offset.unwrap_or(0)));
        if let Some(limit) = limit {
            statement.limit(page_bound(limit));
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        self.stored_states(&sql, values).await
    }

    /// Run a statement that selects the `payload` column and decode every row
    /// it returns into the entity state it stores.
    async fn stored_states(&self, sql: &str, values: SqlxValues) -> Result<Vec<EntityState>, ContextStoreError> {
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.iter().map(payload_of).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::payload_listing::page_bound;
    use pretty_assertions::assert_eq;

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
}
