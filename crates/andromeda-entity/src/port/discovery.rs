//! Entity-type and attribute discovery reads (clauses 5.7.5 to 5.7.10).

use crate::{attribute_summary::AttributeSummary, entity_type_summary::EntityTypeSummary, error::EntityError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use futures::future::BoxFuture;

/// One entity type and the attributes carried by entities of that type.
pub type EntityTypeAttributes = (EntityTypeName, Vec<AttributeName>);

/// One attribute and the types of the entities carrying it.
pub type AttributeEntityTypes = (AttributeName, Vec<EntityTypeName>);

/// Reads what the existing entities carry, as clause 5.7 discovery reports it.
///
/// Every answer covers only what exists when it is read: a type no entity
/// carries any more and an attribute no instance is left of are absent.
pub trait EntityDiscoveryStore: Send + Sync + 'static {
    /// Every entity type at least one entity carries, in name order.
    fn types<'life0, 'life1, 'async_trait>(&'life0 self, target: &'life1 ContextTarget) -> BoxFuture<'async_trait, Result<Vec<EntityTypeName>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Every entity type with the attributes its entities carry, in type name order.
    fn entity_type_attributes<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<Vec<EntityTypeAttributes>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// What the entities carrying one type hold, or `None` when none does.
    fn entity_type_summary<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        entity_type: &'life2 EntityTypeName,
    ) -> BoxFuture<'async_trait, Result<Option<EntityTypeSummary>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Every attribute with the types of the entities carrying it, in attribute name order.
    fn attribute_entity_types<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<Vec<AttributeEntityTypes>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// What the entities carry of one attribute, or `None` when no instance of it exists.
    fn attribute_summary<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        attribute: &'life2 AttributeName,
    ) -> BoxFuture<'async_trait, Result<Option<AttributeSummary>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
