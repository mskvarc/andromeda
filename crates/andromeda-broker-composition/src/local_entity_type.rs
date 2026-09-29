//! The federation write path's local-entity-type resolver, backed by the
//! tenant's entity reader.
//!
//! The federation service decides whether a registration covers a write by the
//! entity's type. An attribute-level write carries no `type` in its request, so
//! without this the write is matched against every registration regardless of
//! the type it declares (clause 5.9.2.4). This adapter reads the local entity's
//! stored, expanded payload and hands its `@type` back so coverage is decided by
//! the type the entity actually has.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::reader::EntityReader;
use andromeda_federation::local_entity_type::LocalEntityTypeResolver;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::node::expanded_object_ref;
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;
use tracing::warn;

/// Resolves a local entity's expanded type through the tenant's entity reader.
pub(crate) struct EntityReaderTypeResolver {
    reader: Arc<dyn EntityReader>,
}

impl EntityReaderTypeResolver {
    /// Wrap the tenant's entity reader as the federation write path's type resolver.
    pub(crate) fn new(reader: Arc<dyn EntityReader>) -> EntityReaderTypeResolver {
        EntityReaderTypeResolver { reader }
    }
}

impl LocalEntityTypeResolver for EntityReaderTypeResolver {
    fn local_entity_types<'resolver>(&'resolver self, target: &'resolver ContextTarget, id: &'resolver Identifier) -> BoxFuture<'resolver, Vec<IriBuf>> {
        Box::pin(async move {
            match self.reader.get(target, id).await {
                Ok(Some(entity)) => expanded_types(&entity),
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

/// Every `@type` IRI of an expanded entity payload. The reader hands back the
/// stored node wrapped in an array, so it is unwrapped first.
fn expanded_types(entity: &Value) -> Vec<IriBuf> {
    let Some(members) = expanded_object_ref(entity) else {
        return Vec::new();
    };
    let Some(types) = members.get("@type").and_then(Value::as_array) else {
        return Vec::new();
    };
    types
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|iri| IriBuf::new(iri.to_owned()).ok())
        .collect()
}
