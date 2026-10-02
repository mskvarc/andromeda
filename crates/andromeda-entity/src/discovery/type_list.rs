//! Retrieve Available Entity Types (clause 5.7.5).

use crate::{
    discovery::{DiscoveryScope, EntityDiscoveryService, registry_union::extend_without_repetition, rendered_names::compacted_type_names},
    entity_invariant::EntityInvariant,
    error::EntityError,
    query::simplified_names::request_name_compaction,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::EntityTypeListId;
use andromeda_ngsi_ld_model::entity_type_list::{EntityTypeList, EntityTypeListType};

impl EntityDiscoveryService {
    /// List the entity types for which instances exist, or possibly exist in
    /// a registered source, as a compacted `EntityTypeList` (clause 5.2.24).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store or the registry
    /// could not be consulted, [`EntityError::JsonLd`] when the request context
    /// does not resolve or process, and [`EntityError::Internal`] when a stored
    /// type name does not read back as one or the minted list identifier is not
    /// one.
    pub async fn types(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<EntityTypeList, EntityError> {
        let id = EntityTypeListId::mint().map_err(EntityInvariant::MintedEntityTypeListId)?;

        let mut type_iris = self.store.types(target).await?;
        extend_without_repetition(
            &mut type_iris,
            self.registered_types(scope).await?.into_iter().map(|registered| registered.name),
        );

        let mut type_list = compacted_type_names(&request_name_compaction(&self.engine, context).await?, type_iris)?;
        type_list.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let type_list = type_list;

        Ok(EntityTypeList {
            id,
            type_: EntityTypeListType,
            type_list,
        })
    }
}
