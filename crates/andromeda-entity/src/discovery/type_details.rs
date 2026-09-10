//! Retrieve Details of Available Entity Types (clause 5.7.6).

use crate::{
    discovery::{DiscoveryScope, EntityDiscoveryService, registry_union::union_type_summaries, stored_names::parse_discovery_iri},
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::entity_type_list::{EntityType, EntityTypeType};
use serde_json::Value;

impl EntityDiscoveryService {
    /// Return detailed information for all entity types, including their
    /// attribute names (clause 5.2.25). A type only the registry knows is
    /// reported with the attribute names registered beside it, which may be
    /// none (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when a summary page or the registry
    /// could not be read, and [`EntityError::Internal`] when a stored type or
    /// attribute name does not read back as one.
    pub async fn type_details(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<Value, EntityError> {
        // Keyset-paginate the aggregation so a very large set of distinct
        // types does not require a single large result set.
        const PAGE: u32 = 500;
        let mut summaries: Vec<(EntityTypeName, Vec<AttributeName>)> = Vec::new();
        let mut cursor: Option<EntityTypeName> = None;
        loop {
            let page = self.store.get_entity_type_summaries(target, cursor.as_ref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|(t, _)| t.clone()); // clone: own cursor for next loop iteration
            let page_len = page.len();
            summaries.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }
        union_type_summaries(&mut summaries, self.registered_types(scope).await?);

        let mut entity_types: Vec<EntityType> = Vec::with_capacity(summaries.len());
        for (type_iri, attrs) in summaries {
            entity_types.push(EntityType {
                id: parse_discovery_iri(type_iri.to_string())?,
                type_name: self.compacted_type_name(&type_iri, context).await?,
                type_: EntityTypeType,
                attribute_names: self.compacted_attribute_names(attrs, context).await?,
            });
        }

        Ok(serde_json::to_value(entity_types)?)
    }
}
