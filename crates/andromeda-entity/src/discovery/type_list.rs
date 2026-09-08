//! Retrieve Available Entity Types (clause 5.7.5).

use crate::{
    discovery::{DiscoveryScope, EntityDiscoveryService, registry_union::extend_without_repetition, stored_names::parse_type_name},
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::options::{CompactOptions, ContextStripping},
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, ngsi_ld_urn::EntityTypeListId, uri::NGSI_LD_URN_NID};
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
    /// consulted, and [`EntityError::JsonLd`] when a type IRI cannot be
    /// compacted against `context`.
    pub async fn types(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<EntityTypeList, EntityError> {
        const TYPE: &str = "EntityTypeList";

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("{}:{}", TYPE, Uuid::new_v4())).build()?;
        let id = EntityTypeListId::try_from(id)?;

        let mut type_iris = self.store.types(target).await?;
        extend_without_repetition(
            &mut type_iris,
            self.registered_types(scope).await?.into_iter().map(|registered| registered.name),
        );

        // Compact each type IRI using a synthetic doc
        let mut type_list = Vec::new();
        for iri in &type_iris {
            type_list.push(self.compact_type_name(iri, context).await?);
        }
        type_list.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        Ok(EntityTypeList {
            id,
            type_: EntityTypeListType,
            type_list,
        })
    }

    /// The short name `context` gives a stored type IRI, or the IRI itself
    /// when the context does not shorten it.
    async fn compact_type_name(&self, iri: &EntityTypeName, context: &ContextSpec) -> Result<EntityTypeName, EntityError> {
        let doc = serde_json::json!({
            "@id": "urn:ngsi-ld:_:dummy",
            "@type": [iri]
        });
        let compacted = self
            .engine
            .compact(
                doc,
                context,
                CompactOptions {
                    strip_context: ContextStripping::Strip,
                },
            )
            .await;
        let name = match &compacted {
            Ok(document) => document.get("type").and_then(|t| t.as_str()).unwrap_or(iri.as_str()),
            Err(_) => iri.as_str(),
        };
        parse_type_name(name)
    }
}
