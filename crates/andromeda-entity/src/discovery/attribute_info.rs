//! Retrieve Available Attribute Information (clause 5.7.10).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        compaction_fixups::{ensure_array_fields, strip_ngsild_prefix},
        registry_union::extend_without_repetition,
        stored_names::parse_attribute_name,
    },
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::attribute_list::AttributeInfo;
use jsonld::Expandable;
use serde_json::Value;

/// What is known about one attribute across the local store and the registry.
struct AttributeFacts {
    /// The clause 5.2.28 attribute families the local instances carry.
    attribute_types: Vec<AttributeType>,
    /// The number of local entities carrying the attribute.
    entity_count: i64,
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
    pub async fn attribute_info(
        &self,
        target: &ContextTarget,
        attr_id: &AttributeName,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri_buf = self.engine.expand_term(attr_id.as_str(), &prepared_context).await?;
        let attr_iri = parse_attribute_name(attr_iri_buf.as_str())?;

        let local = self.store.get_attribute_info(target, &attr_iri).await?;
        let registered = self
            .registered_attributes(scope)
            .await?
            .into_iter()
            .find(|registered| registered.name == attr_iri);
        let facts = match (local, registered) {
            (None, None) => return Err(EntityError::attribute_not_found(attr_id)),
            (Some(row), None) => AttributeFacts {
                attribute_types: row.attribute_types,
                entity_count: row.entity_count,
                entity_types: row.entity_types,
            },
            (None, Some(registered)) => AttributeFacts {
                attribute_types: Vec::new(),
                entity_count: 0,
                entity_types: registered.entity_types,
            },
            (Some(row), Some(registered)) => {
                let mut entity_types = row.entity_types;
                extend_without_repetition(&mut entity_types, registered.entity_types);
                AttributeFacts {
                    attribute_types: row.attribute_types,
                    entity_count: row.entity_count,
                    entity_types,
                }
            }
        };

        // Clause 5.2.28 fixes `attributeTypes` to the attribute family names; the
        // store already mapped the payload's expanded `@type` IRIs to them.
        let attr = AttributeInfo::builder()
            .attribute_name(attr_iri)
            .id(attr_iri_buf)
            .attribute_types(facts.attribute_types)
            .attribute_count(Some(facts.entity_count))
            .type_names(Some(facts.entity_types))
            .build();

        let mut expanded_obj = attr.expand::<serde_json::Value>();
        if let Some(obj) = expanded_obj.as_object_mut() {
            obj.retain(|_, v| match v {
                Value::Array(arr) => !arr.is_empty(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => true,
            });
        }

        let expanded = serde_json::json!([expanded_obj]);
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;
        strip_ngsild_prefix(&mut compacted);
        ensure_array_fields(&mut compacted, &["attributeTypes", "typeNames"]);
        Ok(compacted)
    }
}
