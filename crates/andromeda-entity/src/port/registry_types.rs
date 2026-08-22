//! The entity types and attributes the Context Registry says are possibly
//! available (clause 5.7.11).

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use futures::future::BoxFuture;

/// One entity type some registration declares, with the attribute names
/// declared beside it.
///
/// Registrations have different granularities (clause 5.7.11): a type may be
/// registered with no attribute at all, in which case the list is empty and
/// the detail the registry cannot give is reported as an empty array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredType {
    /// The expanded entity type IRI.
    pub name: EntityTypeName,
    /// The expanded attribute names registered for the type, without repetition.
    pub attribute_names: Vec<AttributeName>,
}

/// One attribute name some registration declares, with the entity types
/// declared beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredAttribute {
    /// The expanded attribute IRI.
    pub name: AttributeName,
    /// The expanded entity type IRIs registered beside the attribute, without
    /// repetition; empty when it was registered without a type.
    pub entity_types: Vec<EntityTypeName>,
}

/// Reads the Context Registry's view of what is possibly available, for the
/// clause 5.7.5 to 5.7.10 retrievals to union with the local store.
pub trait RegistryTypeSource: Send + Sync + 'static {
    /// Every entity type an unexpired registration declares.
    fn registered_types<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<RegisteredType>, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Every attribute name an unexpired registration declares.
    fn registered_attributes<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<RegisteredAttribute>, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
