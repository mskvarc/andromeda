//! Resolving, once per request, the expanded IRI of every attribute key a
//! response carries.
//!
//! Matching a clause 4.21 projection term against an attribute key
//! needs the request `@context`, which only this layer holds. Whatever applies
//! a projection further out — the renderer nesting linked entities above all —
//! receives the expansions already resolved instead of a JSON-LD engine.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;

impl EntityQueryService {
    /// The expanded IRI of every attribute key the given entities carry.
    ///
    /// Expansion is memoised by the engine's term cache, so a key repeated
    /// across entities costs one lookup.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub async fn attribute_iri_index<'a>(
        &self,
        entities: impl IntoIterator<Item = &'a Entity>,
        context: &ContextSpec,
    ) -> Result<AttributeIriIndex, EntityError> {
        let mut index = AttributeIriIndex::new();
        for entity in entities {
            for key in entity.attrs.keys() {
                let expanded = self.expanded_key(key.as_str(), context).await?;
                index.insert(key.clone(), expanded); // clone: the index owns the keys it maps
            }
        }
        Ok(index)
    }
}
