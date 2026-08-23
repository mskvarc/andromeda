//! Entity-type and attribute discovery reads.

use crate::{attribute_summary::AttributeSummary, error::EntityError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use futures::future::BoxFuture;
use serde_json::Value;

/// One entity type and the attributes carried by entities of that type.
pub type EntityTypeAttributes = (EntityTypeName, Vec<AttributeName>);

/// Reads the aggregates and payload pages used by clause 5.7 discovery.
pub trait EntityDiscoveryStore: Send + Sync + 'static {
    /// List every stored entity type.
    fn types<'life0, 'life1, 'async_trait>(&'life0 self, target: &'life1 ContextTarget) -> BoxFuture<'async_trait, Result<Vec<EntityTypeName>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Read one ordered page of entity-type summaries.
    fn get_entity_type_summaries<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        after: Option<&'life2 EntityTypeName>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityTypeAttributes>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read one ordered page of attribute summaries.
    fn get_attribute_summary<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        after: Option<&'life2 AttributeName>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<AttributeSummary>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read one attribute summary.
    fn get_attribute_info<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        attribute: &'life2 AttributeName,
    ) -> BoxFuture<'async_trait, Result<Option<AttributeSummary>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read expanded entity payloads carrying one type.
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
    /// Read all expanded entity payloads with pagination.
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
    /// Count entities carrying one type.
    fn count_by_type<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        entity_type: &'life2 EntityTypeName,
    ) -> BoxFuture<'async_trait, Result<i64, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
