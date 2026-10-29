//! Reading whole Entities in bulk, by type or across the whole space.
//!
//! Neither answer aggregates: both read the head of every Entity the caller
//! asked for, ordered by entity id and continued after the last id of the
//! previous page, and then the rows of the Entities the page kept.

use crate::{
    current_attribute::read::RowSelection,
    entity::{entity_types::Search, repo::EntityRepository, stored_entities::heads_statement},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_pool::read_share::PoolLane;
use andromeda_postgresql_schema::current::{E, Entities, EntitySearch};
use andromeda_postgresql_seaquery::{operator::array_contains_key, statement::Statement};
use sea_query::{Expr, ExprTrait, Order, SelectStatement};
use std::{slice::from_ref, sync::Arc};

impl EntityRepository {
    /// Retrieve one page of the stored states of the entities of one type, in
    /// identifier order, from the first identifier after `after`.
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
        after: Option<&Identifier>,
        limit: u16,
    ) -> Result<Vec<EntityState>, ContextStoreError> {
        // A type the dictionary never interned was never carried by any entity.
        let Some(type_key) = self.entity_type_names.lookup(from_ref(entity_type)).await?.get(entity_type) else {
            return Ok(Vec::new());
        };
        let mut statement = heads_statement(target);
        statement.and_where(array_contains_key(Expr::col((Search, EntitySearch::EntityTypeNameKeys)), type_key.get()));
        self.stored_states(target, keyset_page(statement, after, limit)).await
    }

    /// Retrieve one page of the stored states of every entity, in identifier
    /// order, from the first identifier after `after`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a read or a name lookup fails, or
    /// when the stored rows no longer read back as entity states.
    pub async fn get_all_entities(&self, target: &ContextTarget, after: Option<&Identifier>, limit: u16) -> Result<Vec<EntityState>, ContextStoreError> {
        self.stored_states(target, keyset_page(heads_statement(target), after, limit)).await
    }

    /// Run a statement selecting heads and read the states of the Entities it
    /// selected, in its order.
    async fn stored_states(&self, target: &ContextTarget, statement: SelectStatement) -> Result<Vec<EntityState>, ContextStoreError> {
        let heads = statement.to_query().fetch_all(&self.pool).await?;
        Ok(self
            .entities_of_heads(PoolLane::Whole, target, &heads, &RowSelection::Every)
            .await?
            .into_iter()
            .map(|retrieved| Arc::unwrap_or_clone(retrieved.state))
            .collect())
    }
}

/// `statement` ordered by entity id, past `after` and at most `limit` long.
///
/// Continuing after an identifier lets the id index seek to the page, where an
/// offset would read and discard every row before it.
fn keyset_page(mut statement: SelectStatement, after: Option<&Identifier>, limit: u16) -> SelectStatement {
    if let Some(after) = after {
        statement.and_where(Expr::col((E, Entities::Id)).gt(after.as_str()));
    }
    statement.order_by((E, Entities::Id), Order::Asc).limit(u64::from(limit));
    statement
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity::{state_listing::keyset_page, stored_entities::heads_statement};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use sea_query::PostgresQueryBuilder;
    use std::str::FromStr;

    #[test]
    fn a_first_page_is_ordered_by_identifier_and_bounded_without_an_offset() {
        let sql = keyset_page(heads_statement(&ContextTarget::Live), None, 10).to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"ORDER BY "e"."id" ASC LIMIT 10"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
        assert!(!sql.contains(r#""e"."id" >"#), "{sql}");
    }

    /// The page after one continues from the last identifier it held.
    #[test]
    fn a_later_page_continues_after_the_previous_last_identifier() {
        let after = Identifier::from_str("urn:ngsi-ld:Vehicle:7").expect("an identifier");
        let sql = keyset_page(heads_statement(&ContextTarget::Live), Some(&after), 10).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""e"."id" > 'urn:ngsi-ld:Vehicle:7'"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "e"."id" ASC LIMIT 10"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
        assert!(!sql.contains("current_attributes"), "{sql}");
    }
}
