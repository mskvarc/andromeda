//! Reading whole entity states a page at a time.

use crate::error::EntityError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use futures::future::BoxFuture;

/// Reads the stored states of every entity, or of every entity carrying one
/// type, in identifier order and one page at a time.
///
/// A page continues after the last identifier of the one before it, so reading
/// a page costs the same however far into the listing it lies.
pub trait EntityPayloadPages: Send + Sync + 'static {
    /// Up to `limit` stored states of the entities carrying one type, from the
    /// first identifier after `after` (from the first entity when `None`).
    fn get_entities_by_type<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        entity_type: &'life2 EntityTypeName,
        after: Option<&'life3 Identifier>,
        limit: u16,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityState>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
    /// Up to `limit` stored states of every entity, from the first identifier
    /// after `after` (from the first entity when `None`).
    fn get_all_entities<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        after: Option<&'life2 Identifier>,
        limit: u16,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityState>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
