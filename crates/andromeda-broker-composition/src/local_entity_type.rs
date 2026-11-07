//! The federation write path's local-entity-type resolver, backed by the
//! tenant's entity store.
//!
//! The federation service decides whether a registration covers a write by the
//! entity's type. An attribute-level write carries no `type` in its request, so
//! without this the write is matched against every registration regardless of
//! the type it declares (clause 5.9.2.4). This adapter reads the local entity's
//! types from the store, which keeps them beside the payload, so coverage is
//! decided by the type the entity actually has without decoding the payload.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::entity_types::EntityTypeReader;
use andromeda_federation::local_entity_type::LocalEntityTypeResolver;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use futures::future::BoxFuture;
use std::sync::Arc;
use tracing::warn;

/// Resolves a local entity's expanded types through the tenant's entity store.
pub(crate) struct EntityStoreTypeResolver {
    /// Reads the stored expanded types of a local entity without its payload.
    types: Arc<dyn EntityTypeReader>,
}

impl EntityStoreTypeResolver {
    /// Wrap the tenant's entity type reader as the federation write path's type resolver.
    pub(crate) fn new(types: Arc<dyn EntityTypeReader>) -> EntityStoreTypeResolver {
        EntityStoreTypeResolver { types }
    }
}

impl LocalEntityTypeResolver for EntityStoreTypeResolver {
    fn local_entity_types<'resolver>(
        &'resolver self,
        target: &'resolver ContextTarget,
        id: &'resolver Identifier,
    ) -> BoxFuture<'resolver, Vec<EntityTypeName>> {
        Box::pin(async move {
            match self.types.entity_types(target, id).await {
                Ok(Some(types)) => types,
                Ok(None) => Vec::new(),
                Err(error) => {
                    // A read failure must not become a spurious refusal: fall back
                    // to id-only matching, and the local write path's own read
                    // surfaces the fault if the store is genuinely unavailable.
                    warn!(entity = %id, error = %error, "federation could not resolve local entity types; matching registrations by id only");
                    Vec::new()
                }
            }
        })
    }
}
