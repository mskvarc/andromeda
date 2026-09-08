//! Retrieve Available Attributes and their details (clauses 5.7.8 and 5.7.9).

use crate::{
    attribute_summary::AttributeSummary,
    discovery::{
        AttributeCollectionRepresentation,
        DiscoveryScope,
        EntityDiscoveryService,
        compaction_fixups::{ensure_array_fields, strip_ngsild_prefix},
        registry_union::extend_without_repetition,
        stored_names::{parse_attribute_name, parse_discovery_iri},
    },
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::options::{CompactOptions, ContextStripping},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, ngsi_ld_urn::AttributeListId, uri::NGSI_LD_URN_NID};
use andromeda_ngsi_ld_model::attribute_list::{AttributeInfo, AttributeList, AttributeListType, AttributeResourceType};
use jsonld::Expandable;
use serde_json::Value;
use std::collections::HashMap;
use urn_rs::UrnBuilder;
use uuid::Uuid;

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
    /// Reports [`EntityError::Persistence`] when a summary page or the registry
    /// could not be read, [`EntityError::Identifier`] or
    /// [`EntityError::AttributeListIdentifier`] when the generated list
    /// identifier is not a well-formed NGSI-LD URN, and
    /// [`EntityError::JsonLd`] when the list cannot be compacted against
    /// `context`.
    pub async fn attributes(
        &self,
        target: &ContextTarget,
        representation: AttributeCollectionRepresentation,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        // Keyset-paginate attribute summary aggregation.
        const PAGE: u32 = 500;
        let mut summary_rows: Vec<AttributeSummary> = Vec::new();
        let mut cursor: Option<AttributeName> = None;
        loop {
            let page = self.store.get_attribute_summary(target, cursor.as_ref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|r| r.attr_iri.clone()); // clone: own cursor for next loop iteration
            let page_len = page.len();
            summary_rows.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }

        // Build the same HashMap structure from SQL aggregation results
        let mut attr_data: HashMap<AttributeName, AttributeMetadata> = HashMap::with_capacity(summary_rows.len());
        for row in summary_rows {
            attr_data.insert(
                row.attr_iri,
                AttributeMetadata {
                    entity_types: row.entity_types,
                },
            );
        }
        for registered in self.registered_attributes(scope).await? {
            let metadata = attr_data
                .entry(registered.name)
                .or_insert_with(|| AttributeMetadata { entity_types: Vec::new() });
            extend_without_repetition(&mut metadata.entity_types, registered.entity_types);
        }

        match representation {
            AttributeCollectionRepresentation::Detailed => self.detailed_attributes(attr_data, context).await,
            AttributeCollectionRepresentation::List => self.attribute_name_list(attr_data.into_keys(), context).await,
        }
    }

    /// `details=true` on `/attributes` returns a simplified view: id, type,
    /// attributeName, typeNames (no attributeTypes/attributeCount).
    async fn detailed_attributes(&self, attr_data: HashMap<AttributeName, AttributeMetadata>, context: &ContextSpec) -> Result<Value, EntityError> {
        let mut attributes = Vec::new();
        for (iri, meta) in attr_data {
            attributes.push(
                AttributeInfo::builder()
                    .attribute_name(iri.clone()) // clone: the record owns its name and its id is derived from the same value
                    .id(parse_discovery_iri(iri.to_string())?)
                    .type_(AttributeResourceType)
                    .attribute_types(Vec::new())
                    .attribute_count(None)
                    .type_names(Some(meta.entity_types))
                    .build(),
            );
        }
        attributes.sort_by(|a, b| a.id.cmp(&b.id));

        let expanded = serde_json::json!(
            attributes
                .iter()
                .map(|a| {
                    let mut val = a.expand::<serde_json::Value>();
                    if let Some(obj) = val.as_object_mut() {
                        obj.retain(|_, v| match v {
                            Value::Array(arr) => !arr.is_empty(),
                            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => true,
                        });
                    }
                    val
                })
                .collect::<Vec<_>>()
        );
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;
        // Extract @graph array (compaction wraps multiple items)
        if let Some(graph) = compacted.as_object_mut().and_then(|obj| obj.remove("@graph")) {
            compacted = graph;
        }
        strip_ngsild_prefix(&mut compacted);
        ensure_array_fields(&mut compacted, &["attributeTypes", "typeNames"]);
        Ok(compacted)
    }

    /// The clause 5.2.27 `AttributeList` of the attribute names, each
    /// compacted under `context`.
    async fn attribute_name_list(&self, attr_iris: impl Iterator<Item = AttributeName>, context: &ContextSpec) -> Result<Value, EntityError> {
        let mut compacted_names: Vec<AttributeName> = Vec::new();
        for iri in attr_iris {
            compacted_names.push(self.compact_attribute_name(&iri, context).await?);
        }
        compacted_names.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("AttributeList:{}", Uuid::new_v4())).build()?;
        let id = AttributeListId::try_from(id)?;

        let list = AttributeList {
            id,
            type_: AttributeListType,
            attribute_list: compacted_names,
        };

        Ok(serde_json::to_value(list)?)
    }

    /// The short name `context` gives a stored attribute IRI, or the IRI
    /// itself when the context does not shorten it.
    async fn compact_attribute_name(&self, iri: &AttributeName, context: &ContextSpec) -> Result<AttributeName, EntityError> {
        let doc = serde_json::json!([{
            "@id": "urn:ngsi-ld:_:dummy",
            (iri.as_str()): [{"@value": "dummy"}]
        }]);
        let name = match self
            .engine
            .compact(
                doc,
                context,
                CompactOptions {
                    strip_context: ContextStripping::Strip,
                },
            )
            .await
        {
            Ok(compacted) => match compacted.as_object() {
                Some(obj) => obj
                    .keys()
                    .find(|k| k.as_str() != "id" && k.as_str() != "type" && !k.starts_with('@'))
                    .cloned() // clone: owned String from borrowed map key
                    .unwrap_or_else(|| iri.to_string()),
                None => iri.to_string(),
            },
            Err(_) => iri.to_string(),
        };
        parse_attribute_name(&name)
    }
}
