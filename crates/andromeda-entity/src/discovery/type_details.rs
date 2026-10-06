//! Retrieve Details of Available Entity Types (clause 5.7.6).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        registry_union::union_type_summaries,
        rendered_names::{compacted_attribute_names, compacted_type_name},
        stored_names::parse_discovery_iri,
    },
    error::EntityError,
    query::simplified_names::request_name_compaction,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity_type_list::{EntityType, EntityTypeType};

impl EntityDiscoveryService {
    /// Return detailed information for all entity types, including their
    /// attribute names (clause 5.2.25). A type only the registry knows is
    /// reported with the attribute names registered beside it, which may be
    /// none (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store or the registry
    /// could not be read, [`EntityError::JsonLd`] when the request context
    /// does not resolve or process, and [`EntityError::Internal`] when a stored
    /// type or attribute name does not read back as one.
    pub async fn type_details(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<Vec<EntityType>, EntityError> {
        let mut summaries = self.store.entity_type_attributes(target).await?;
        union_type_summaries(&mut summaries, self.registered_types(scope).await?);

        let names = request_name_compaction(&self.engine, context).await?;
        let mut entity_types: Vec<EntityType> = Vec::with_capacity(summaries.len());
        for (type_iri, attrs) in summaries {
            entity_types.push(EntityType {
                id: parse_discovery_iri(type_iri.to_string())?,
                type_name: compacted_type_name(&names, &type_iri)?,
                type_: EntityTypeType,
                attribute_names: compacted_attribute_names(&names, attrs)?,
            });
        }

        Ok(entity_types)
    }
}
