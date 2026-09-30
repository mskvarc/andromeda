//! Retrieve Available Attributes and their details (clauses 5.7.8 and 5.7.9).

use crate::{
    discovery::{
        AttributeCollectionRepresentation,
        DiscoveryScope,
        EntityDiscoveryService,
        registry_union::extend_without_repetition,
        rendered_names::{compacted_attribute_name, compacted_attribute_names, compacted_type_names},
        stored_names::parse_discovery_iri,
    },
    entity_invariant::EntityInvariant,
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, ngsi_ld_urn::AttributeListId};
use andromeda_ngsi_ld_model::attribute_list::{AttributeInfo, AttributeList, AttributeListType, AttributeResourceType};
use serde_json::Value;
use std::collections::HashMap;

/// The entity types an attribute is known to belong to, across the local
/// store and the registry.
struct AttributeMetadata {
    entity_types: Vec<EntityTypeName>,
}

impl EntityDiscoveryService {
    /// List all known attributes, optionally including per-attribute detail.
    /// An attribute only the registry knows is reported with the entity types
    /// registered beside it, which may be none (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the store or the registry
    /// could not be read, and [`EntityError::Internal`] when a stored name does
    /// not read back as one or the minted list identifier is not one.
    pub async fn attributes(
        &self,
        target: &ContextTarget,
        representation: AttributeCollectionRepresentation,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        let local = self.store.attribute_entity_types(target).await?;
        let mut attr_data: HashMap<AttributeName, AttributeMetadata> = HashMap::with_capacity(local.len());
        for (attribute, entity_types) in local {
            attr_data.insert(attribute, AttributeMetadata { entity_types });
        }
        for registered in self.registered_attributes(scope).await? {
            let metadata = attr_data
                .entry(registered.name)
                .or_insert_with(|| AttributeMetadata { entity_types: Vec::new() });
            extend_without_repetition(&mut metadata.entity_types, registered.entity_types);
        }

        let names = NameCompactor::new(&self.engine, context);
        match representation {
            AttributeCollectionRepresentation::Detailed => detailed_attributes(attr_data, &names).await,
            AttributeCollectionRepresentation::List => attribute_name_list(attr_data.into_keys(), &names).await,
        }
    }
}

/// The clause 4.5.14 detailed attribute list: one record per attribute,
/// carrying the full URI of the name, the short name the request's
/// `@context` gives it, and the entity types it appears on.
///
/// Table 5.2.28-1 restricts `attributeCount` and `attributeTypes` to
/// cardinality 0..1 and clause 5.7.9.5 asks only for the names, so neither
/// is reported here; clause 5.7.10 is where an attribute's families and
/// count are answered.
async fn detailed_attributes(attr_data: HashMap<AttributeName, AttributeMetadata>, names: &NameCompactor<'_>) -> Result<Value, EntityError> {
    let mut attributes = Vec::with_capacity(attr_data.len());
    for (iri, meta) in attr_data {
        attributes.push(
            AttributeInfo::builder()
                .attribute_name(compacted_attribute_name(names, &iri).await?)
                .id(parse_discovery_iri(iri.to_string())?)
                .type_(AttributeResourceType)
                .type_names(compacted_type_names(names, meta.entity_types).await?)
                .build(),
        );
    }
    attributes.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(serde_json::to_value(attributes)?)
}

/// The clause 5.2.27 `AttributeList` of the attribute names, each
/// compacted under the context `names` compacts against.
async fn attribute_name_list(attr_iris: impl IntoIterator<Item = AttributeName>, names: &NameCompactor<'_>) -> Result<Value, EntityError> {
    let mut compacted_names = compacted_attribute_names(names, attr_iris).await?;
    compacted_names.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let compacted_names = compacted_names;

    let id = AttributeListId::mint().map_err(EntityInvariant::MintedAttributeListId)?;

    let list = AttributeList {
        id,
        type_: AttributeListType,
        attribute_list: compacted_names,
    };

    Ok(serde_json::to_value(list)?)
}
