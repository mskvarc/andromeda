//! Reading whole entity payloads a page at a time.

use crate::error::EntityError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use futures::future::BoxFuture;
use serde_json::Value;

/// Reads the expanded payloads of every entity, or of every entity carrying
/// one type, in identifier order and one page at a time.
pub trait EntityPayloadPages: Send + Sync + 'static {
    /// One page of the expanded payloads of the entities carrying one type.
    fn get_entities_by_type<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        entity_type: &'life2 EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Value>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// One page of the expanded payloads of every entity.
    fn get_all_entities<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Value>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
