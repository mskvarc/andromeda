//! The types an entity carries, read without its payload.

use crate::error::EntityError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use futures::future::BoxFuture;

/// Reads the expanded types of one stored entity.
///
/// Deciding which registrations cover a write needs the entity's types
/// (clause 5.9.2.4) and nothing else of it; the store keeps them beside the
/// payload, so the answer never decodes the payload.
pub trait EntityTypeReader: Send + Sync + 'static {
    /// The expanded types of the entity `id` names, or `None` when there is
    /// none.
    fn entity_types<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Vec<EntityTypeName>>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
