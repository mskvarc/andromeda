//! Retrieve Available Attribute Information (clause 5.7.10).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        registry_union::extend_without_repetition,
        rendered_names::{compacted_attribute_name, compacted_type_names},
        stored_names::parse_attribute_name,
    },
    error::EntityError,
    query::simplified_names::request_name_compaction,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::attribute_list::AttributeInfo;
use serde_json::Value;

/// What is known about one attribute across the local store and the registry.
struct AttributeFacts {
    /// The clause 5.2.28 attribute families the local instances carry.
    attribute_types: Vec<AttributeType>,
    /// Table 5.2.28-1 `attributeCount`: the number of local instances of the
    /// attribute.
    instance_count: u64,
    /// The entity types carrying or registering the attribute.
    entity_types: Vec<EntityTypeName>,
}

impl EntityDiscoveryService {
    /// Return detailed information for a single attribute (clause 5.2.28):
    /// its attribute types and the count and types of the entities carrying it.
    ///
    /// The attribute is not found only when neither the local store nor the
    /// registry knows it. One only the registry knows is carried by no local
    /// entity, so its count is zero, its attribute types are empty and its
    /// type names are those registered beside it (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `attr_id` cannot be expanded
    /// against `context` or `context` does not resolve or process, [`EntityError::Internal`] when a stored name does not
    /// read back as one, [`EntityError::AttributeNotFound`] when neither the
    /// local store nor the registry knows the attribute, and
    /// [`EntityError::Persistence`] when the store or the registry could not be
    /// consulted.
    pub async fn attribute_info(
        &self,
        target: &ContextTarget,
        attr_id: &AttributeName,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let names = request_name_compaction(&self.engine, context).await?;
        let attr_iri_buf = self.engine.expand_term(attr_id.as_str(), &prepared_context).await?;
        let attr_iri = parse_attribute_name(attr_iri_buf.as_str())?;

        let local = self.store.attribute_summary(target, &attr_iri).await?;
        let registered = self
            .registered_attributes(scope)
            .await?
            .into_iter()
            .find(|registered| registered.name == attr_iri);
        let facts = match (local, registered) {
            (None, None) => return Err(EntityError::attribute_not_found(attr_id)),
            (Some(row), None) => AttributeFacts {
                attribute_types: row.attribute_types,
                instance_count: row.instance_count,
                entity_types: row.entity_types,
            },
            (None, Some(registered)) => AttributeFacts {
                attribute_types: Vec::new(),
                instance_count: 0,
                entity_types: registered.entity_types,
            },
            (Some(row), Some(registered)) => {
                let mut entity_types = row.entity_types;
                extend_without_repetition(&mut entity_types, registered.entity_types);
                AttributeFacts {
                    attribute_types: row.attribute_types,
                    instance_count: row.instance_count,
                    entity_types,
                }
            }
        };

        // Clause 5.2.28 fixes `attributeTypes` to the attribute family names; the
        // store already mapped the payload's expanded `@type` IRIs to them.
        let attr = AttributeInfo::builder()
            .attribute_name(compacted_attribute_name(&names, &attr_iri)?)
            .id(attr_iri_buf)
            .attribute_types(facts.attribute_types)
            .attribute_count(Some(facts.instance_count))
            .type_names(compacted_type_names(&names, facts.entity_types)?)
            .build();

        Ok(serde_json::to_value(attr)?)
    }
}
