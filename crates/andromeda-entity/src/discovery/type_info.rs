//! Retrieve Available Entity Type Information (clause 5.7.7).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        rendered_names::{compacted_attribute_name, compacted_type_name},
        stored_names::{parse_discovery_iri, parse_type_name},
    },
    entity_type_summary::AttributeFamilies,
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    attribute_list::{AttributeDetail, AttributeResourceType},
    entity_type_list::{EntityTypeInfo, EntityTypeInfoType},
};
use serde_json::Value;

impl EntityDiscoveryService {
    /// Return detailed information for a single entity type (clause 5.2.26):
    /// the number of entities carrying it and the attributes those entities
    /// carry, each with the families its instances on them declare.
    ///
    /// The type is not found only when neither the local store nor the
    /// registry knows it. A type only the registry knows has no instances
    /// here, so its count is zero and its attribute details carry the names
    /// registered beside it with no attribute types (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `type_name` cannot be expanded
    /// against `context`, [`EntityError::Internal`] when a stored name does not
    /// read back as one, [`EntityError::TypeNotFound`] when neither the local
    /// store nor the registry knows the type, and [`EntityError::Persistence`]
    /// when the store or the registry could not be consulted.
    pub async fn type_info(
        &self,
        target: &ContextTarget,
        type_name: &EntityTypeName,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let names = NameCompactor::new(&self.engine, context);
        let type_iri = self.engine.expand_type(type_name.as_str(), &prepared_context).await?;
        let type_key = parse_type_name(type_iri.as_str())?;

        let local = self.store.entity_type_summary(target, &type_key).await?;
        let registered = self.registered_types(scope).await?.into_iter().find(|registered| registered.name == type_key);
        let (entity_count, mut attributes) = match (local, registered) {
            (None, None) => return Err(EntityError::type_not_found(type_name)),
            (Some(local), None) => (local.entity_count, local.attributes),
            (None, Some(registered)) => (0, possibly_available(registered.attribute_names)),
            (Some(local), Some(registered)) => {
                let mut attributes = local.attributes;
                // Clause 5.7.11: a registered attribute the instances do not
                // show is possibly available, reported with the families the
                // registry cannot give left empty.
                let unseen: Vec<_> = registered
                    .attribute_names
                    .into_iter()
                    .filter(|name| !attributes.iter().any(|attribute| attribute.name == *name))
                    .collect();
                attributes.extend(possibly_available(unseen));
                (local.entity_count, attributes)
            }
        };
        attributes.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
        let attributes = attributes;

        let mut attribute_details: Vec<AttributeDetail> = Vec::with_capacity(attributes.len());
        for attribute in attributes {
            attribute_details.push(AttributeDetail {
                attribute_name: compacted_attribute_name(&names, &attribute.name).await?,
                id: parse_discovery_iri(attribute.name.to_string())?,
                type_: AttributeResourceType,
                attribute_types: attribute.families,
            });
        }

        let type_info = EntityTypeInfo::builder()
            .id(type_iri)
            .type_(EntityTypeInfoType)
            .type_name(compacted_type_name(&names, &type_key).await?)
            .entity_count(entity_count)
            .attribute_details(attribute_details)
            .build();

        Ok(serde_json::to_value(type_info)?)
    }
}

/// Registered attribute names the registry cannot give families for.
fn possibly_available(names: Vec<AttributeName>) -> Vec<AttributeFamilies> {
    names.into_iter().map(|name| AttributeFamilies { name, families: Vec::new() }).collect()
}
