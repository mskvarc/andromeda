//! Retrieve Available Entity Types (clause 5.7.5).

use crate::{
    discovery::{DiscoveryScope, EntityDiscoveryService, registry_union::extend_without_repetition, rendered_names::compacted_type_names},
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::EntityTypeListId, uri::NGSI_LD_URN_NID};
use andromeda_ngsi_ld_model::entity_type_list::{EntityTypeList, EntityTypeListType};
use urn_rs::UrnBuilder;
use uuid::Uuid;

impl EntityDiscoveryService {
    /// List the entity types for which instances exist, or possibly exist in
    /// a registered source, as a compacted `EntityTypeList` (clause 5.2.24).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Identifier`] or
    /// [`EntityError::EntityTypeListIdentifier`] when the generated list
    /// identifier is not a well-formed NGSI-LD URN,
    /// [`EntityError::Persistence`] when the store or the registry could not be
    /// consulted, and [`EntityError::Internal`] when a stored type name does
    /// not read back as one.
    pub async fn types(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<EntityTypeList, EntityError> {
        const TYPE: &str = "EntityTypeList";

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("{}:{}", TYPE, Uuid::new_v4())).build()?;
        let id = EntityTypeListId::try_from(id)?;

        let mut type_iris = self.store.types(target).await?;
        extend_without_repetition(
            &mut type_iris,
            self.registered_types(scope).await?.into_iter().map(|registered| registered.name),
        );

        let mut type_list = compacted_type_names(&NameCompactor::new(&self.engine, context), type_iris).await?;
        type_list.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let type_list = type_list;

        Ok(EntityTypeList {
            id,
            type_: EntityTypeListType,
            type_list,
        })
    }
}
