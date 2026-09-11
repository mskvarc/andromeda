//! Reading whole entity payloads in bulk, by type or across the whole space.
//!
//! Neither answer aggregates: both project the stored document of every entity
//! the caller asked for, ordered by entity id so an offset names a stable
//! window.

use crate::{
    entity::{read::identity_with_documents, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::operator::array_contains_key;
use sea_query::{Expr, ExprTrait, JoinType, Order, PostgresQueryBuilder};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;

/// The unsigned page bound a signed `limit` or `offset` names.
///
/// A negative value is not a narrower bound but an absent one, so it clamps to
/// zero rather than wrapping into an enormous unsigned width.
fn page_bound(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

impl EntityRepository {
    /// Retrieve entity payloads filtered by type with optional pagination.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the dictionary lookup for the type or
    /// the payload query fails.
    pub async fn get_entities_by_type(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Value>, ContextStoreError> {
        let Some(type_id) = self.type_id(entity_type).await? else {
            return Ok(Vec::new());
        };
        let mut statement = identity_with_documents();
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .join(
                JoinType::Join,
                EntitySearch::Table,
                Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
            )
            .and_where(Expr::col((Entities::Table, Entities::ContextSpaceKey)).eq(target.key().as_i64()))
            .and_where(Expr::col((EntityDocuments::Table, EntityDocuments::ContextSpaceKey)).eq(target.key().as_i64()))
            .and_where(Expr::col((EntitySearch::Table, EntitySearch::ContextSpaceKey)).eq(target.key().as_i64()))
            .and_where(array_contains_key(Expr::col((EntitySearch::Table, EntitySearch::TypeIds)), type_id.get()))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(page_bound(offset.unwrap_or(0)));
        if let Some(limit) = limit {
            statement.limit(page_bound(limit));
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?)
    }

    /// Retrieve all entity payloads with optional pagination.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the payload query fails.
    pub async fn get_all_entities(&self, target: &ContextTarget, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, ContextStoreError> {
        let mut statement = identity_with_documents();
        statement
            .column((EntityDocuments::Table, EntityDocuments::Payload))
            .and_where(Expr::col((Entities::Table, Entities::ContextSpaceKey)).eq(target.key().as_i64()))
            .and_where(Expr::col((EntityDocuments::Table, EntityDocuments::ContextSpaceKey)).eq(target.key().as_i64()))
            .order_by((Entities::Table, Entities::Id), Order::Asc)
            .offset(page_bound(offset.unwrap_or(0)));
        if let Some(limit) = limit {
            statement.limit(page_bound(limit));
        }
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::discovery::entity_listing::page_bound;

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
