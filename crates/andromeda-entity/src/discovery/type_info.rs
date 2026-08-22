//! Retrieve Available Entity Type Information (clause 5.7.7).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        compaction_fixups::{ensure_array_fields, strip_ngsild_prefix},
        stored_names::{extract_attribute_types, parse_discovery_iri, parse_type_name},
    },
    error::EntityError,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    attribute_list::{AttributeDetail, AttributeResourceType},
    entity_type_list::{EntityTypeInfo, EntityTypeInfoType},
};
use jsonld::Expandable;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

impl EntityDiscoveryService {
    /// Return detailed information for a single entity type (clause 5.2.26):
    /// its entity count and the details of the attributes its instances have.
    ///
    /// The type is not found only when neither the local store nor the
    /// registry knows it. A type only the registry knows has no instances
    /// here, so its count is zero and its attribute details carry the names
    /// registered beside it with no attribute types (clause 5.7.11).
    pub async fn type_info(&self, type_name: &EntityTypeName, scope: DiscoveryScope, context: &ContextSpec) -> Result<Value, EntityError> {
        // Expand the type name first — DB stores expanded IRIs
        let prepared_context = self.engine.prepare_term_context(context);
        let type_iri = self.engine.expand_type(type_name.as_str(), &prepared_context).await?;
        let type_key = parse_type_name(type_iri.as_str())?;

        let entities = self.store.get_entities_by_type(&type_key, None, None).await?;
        let count = self.store.count_by_type(&type_key).await?;
        let registered = self.registered_types(scope).await?.into_iter().find(|registered| registered.name == type_key);

        if count == 0 && registered.is_none() {
            return Err(EntityError::type_not_found(type_name));
        }

        // Extract unique attributes from all entities, keyed by their expanded
        // attribute IRI. A key that is not a valid attribute name is not an
        // attribute and is skipped.
        let mut attributes: HashMap<AttributeName, HashSet<AttributeType>> = HashMap::with_capacity(entities.len().saturating_mul(4));

        for entity in &entities {
            // Handle array wrapper from expanded JSON-LD (expand always returns an array)
            let entity_obj = match entity {
                Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
                Value::Object(obj) => Some(obj),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
            };

            if let Some(map) = entity_obj {
                for (key, value) in map {
                    // Skip JSON-LD keywords
                    if key.starts_with('@') {
                        continue;
                    }
                    let Ok(attr_name) = AttributeName::from_str(key) else {
                        continue;
                    };
                    // Extract attribute type from @type field
                    if let Some(attr_types) = extract_attribute_types(value) {
                        attributes.entry(attr_name).or_default().extend(attr_types);
                    }
                }
            }
        }
        // Clause 5.7.11: a registered attribute the instances do not show is
        // possibly available, reported with the families the registry cannot
        // give left empty.
        for name in registered.into_iter().flat_map(|registered| registered.attribute_names) {
            attributes.entry(name).or_default();
        }

        // Build typed attribute details
        let mut attribute_details: Vec<AttributeDetail> = Vec::new();
        for (name, types) in &attributes {
            attribute_details.push(AttributeDetail {
                id: parse_discovery_iri(name.to_string())?,
                type_: AttributeResourceType,
                attribute_name: name.clone(), // clone: the map owns the key and the record owns its name
                attribute_types: types.iter().copied().collect(),
            });
        }

        // Sort for consistent output
        attribute_details.sort_by(|a, b| a.id.cmp(&b.id));

        // Build typed response
        let type_info = EntityTypeInfo::builder()
            .id(type_iri)
            .type_(EntityTypeInfoType)
            .type_name(parse_type_name(type_name)?)
            .entity_count(count)
            .attribute_details(attribute_details)
            .build();

        // Convert to expanded JSON-LD using the trait, then compact
        let expanded = serde_json::json!([type_info.expand::<serde_json::Value>()]);
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        strip_ngsild_prefix(&mut compacted);
        ensure_array_fields(&mut compacted, &["attributeTypes"]);

        Ok(compacted)
    }
}
