use crate::{
    core::error::AndromedaError,
    domain::{
        ngsi_ld_urn::{AttributeListId, EntityTypeListId},
        types::{AttributeDetail, EntityType, EntityTypeInfo, Types},
        uri::NGSI_LD_URN_NID,
    },
    jsonld::{CompactOptions, ContextSpec, ContextStripping, JsonLdEngine},
    repository::entity::{repo::EntityRepository, rows::AttributeSummaryRow},
};
use jsonld::Expandable;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::collections::{HashMap, HashSet};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Service for NGSI-LD entity type and attribute discovery operations.
pub struct EntityDiscoveryService {
    repo: EntityRepository,
    engine: JsonLdEngine,
}

impl EntityDiscoveryService {
    /// Create a new entity discovery service backed by the given connection pool.
    pub fn new(pool: &PgPool, engine: JsonLdEngine) -> EntityDiscoveryService {
        let repo = EntityRepository::new(pool);
        EntityDiscoveryService { repo, engine }
    }

    /// List all known entity types as a compacted `EntityTypeList`.
    pub async fn types(&self, context: &ContextSpec) -> Result<Types, AndromedaError> {
        const TYPE: &str = "EntityTypeList";

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("{}:{}", TYPE, Uuid::new_v4())).build()?;
        let id = EntityTypeListId::try_from(id)?;

        let type_iris = self.repo.types().await?;

        // Compact each type IRI using a synthetic doc
        let mut type_list = Vec::new();
        for iri in &type_iris {
            let doc = serde_json::json!({
                "@id": "urn:ngsi-ld:_:dummy",
                "@type": [iri]
            });
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
                Ok(compacted) => compacted.get("type").and_then(|t| t.as_str()).map_or_else(|| iri.clone(), String::from), // clone: fallback owned copy when compaction lacks type
                Err(_) => iri.clone(), // clone: fallback owned copy when compaction fails
            };
            type_list.push(name);
        }
        type_list.sort();

        let result = Types {
            id,
            type_: TYPE.to_owned(),
            type_list,
        };

        Ok(result)
    }

    /// Return detailed information for all entity types, including their attribute names.
    pub async fn type_details(&self, context: &ContextSpec) -> Result<Value, AndromedaError> {
        // Keyset-paginate the aggregation so a very large set of distinct
        // types does not require a single large result set.
        const PAGE: u32 = 500;
        let mut summaries: Vec<(String, Vec<String>)> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.repo.get_entity_type_summaries(cursor.as_deref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|(t, _)| t.clone()); // clone: own cursor string for next loop iteration
            let page_len = page.len();
            summaries.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }

        let entity_types: Vec<EntityType> = summaries
            .into_iter()
            .map(|(type_iri, attrs)| EntityType {
                id: type_iri.clone(), // clone: value also needed for type_name field below
                type_: "EntityType".to_string(),
                type_name: type_iri,
                attribute_names: attrs,
            })
            .collect();

        // Expand and compact
        let expanded = serde_json::json!(entity_types.iter().map(|et| et.expand::<serde_json::Value>()).collect::<Vec<_>>());
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        // Extract the @graph array (compaction wraps multiple items in @graph)
        if let Some(graph) = compacted.as_object_mut().and_then(|obj| obj.remove("@graph")) {
            compacted = graph;
        }

        strip_ngsild_prefix(&mut compacted);

        Ok(compacted)
    }

    /// Return detailed information for a single entity type, including attribute details and entity count.
    pub async fn type_info(&self, type_name: &str, context: &ContextSpec) -> Result<Value, AndromedaError> {
        // Expand the type name first — DB stores expanded IRIs
        let type_iri = self.engine.expand_type(type_name, context).await?;

        let entities = self.repo.get_entities_by_type(&type_iri, None, None).await?;
        let count = self.repo.count_by_type(&type_iri).await?;

        if count == 0 {
            return Err(AndromedaError::entity_not_found(type_name.to_string()));
        }

        // Extract unique attributes from all entities
        // Key: expanded IRI, Value: set of attribute types (Property, Relationship, etc.)
        let mut attributes: HashMap<String, HashSet<String>> = HashMap::with_capacity(entities.len().saturating_mul(4));

        for entity in &entities {
            // Handle array wrapper from expanded JSON-LD (expand always returns an array)
            let entity_obj = match entity {
                Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
                Value::Object(obj) => Some(obj),
                _other => None,
            };

            if let Some(map) = entity_obj {
                for (key, value) in map {
                    // Skip JSON-LD keywords
                    if key.starts_with('@') {
                        continue;
                    }
                    // Extract attribute type from @type field
                    if let Some(attr_types) = extract_attribute_types(value) {
                        attributes.entry(key.clone()).or_default().extend(attr_types); // clone: owned key for HashMap entry
                    }
                }
            }
        }

        // Build typed attribute details
        let mut attribute_details: Vec<AttributeDetail> = Vec::new();
        for (uri, types) in &attributes {
            attribute_details.push(AttributeDetail {
                id: uri.clone(), // clone: value also needed for attribute_name field
                type_: "Attribute".to_string(),
                attribute_name: uri.clone(),                      // clone: separate owned field from borrowed iterator
                attribute_types: types.iter().cloned().collect(), // clone: owned strings from HashSet references
            });
        }

        // Sort for consistent output
        attribute_details.sort_by(|a, b| a.id.cmp(&b.id));

        // Build typed response
        let type_info = EntityTypeInfo {
            id: type_iri,
            type_: "EntityTypeInfo".to_string(),
            type_name: type_name.to_string(),
            entity_count: count,
            attribute_details,
        };

        // Convert to expanded JSON-LD using the trait, then compact
        let expanded = serde_json::json!([type_info.expand::<serde_json::Value>()]);
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        strip_ngsild_prefix(&mut compacted);
        ensure_array_fields(&mut compacted, &["attributeTypes"]);

        Ok(compacted)
    }

    /// List all known attributes, optionally including per-attribute detail.
    pub async fn attributes(&self, details: bool, context: &ContextSpec) -> Result<Value, AndromedaError> {
        // Keyset-paginate attribute summary aggregation.
        const PAGE: u32 = 500;
        let mut summary_rows: Vec<AttributeSummaryRow> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.repo.get_attribute_summary(cursor.as_deref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|r| r.attr_iri.clone()); // clone: own cursor string for next loop iteration
            let page_len = page.len();
            summary_rows.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }

        // Build the same HashMap structure from SQL aggregation results
        let mut attr_data: HashMap<String, AttributeMetadata> = HashMap::with_capacity(summary_rows.len());
        for row in &summary_rows {
            attr_data.insert(
                row.attr_iri.clone(), // clone: owned key for HashMap entry
                AttributeMetadata {
                    entity_types: row.entity_types.iter().cloned().collect(), // clone: owned strings from borrowed Vec
                },
            );
        }

        if details {
            // details=true on /attributes returns a simplified view:
            // id, type, attributeName, typeNames (no attributeTypes/attributeCount)
            let mut attributes = Vec::new();
            for (iri, meta) in attr_data {
                let iri: String = iri;
                attributes.push(crate::domain::types::Attribute {
                    id: iri.clone(), // clone: value also needed for attribute_name field
                    type_: "Attribute".to_string(),
                    attribute_name: iri.clone(), // clone: separate owned field, iri consumed by later field would not suffice
                    attribute_types: Vec::new(),
                    attribute_count: None,
                    type_names: Some(meta.entity_types.into_iter().collect()),
                });
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
                                _other => true,
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
        } else {
            let attr_iris: Vec<String> = attr_data.into_keys().collect();

            // Compact each attribute IRI using a synthetic expanded doc
            let mut compacted_names: Vec<String> = Vec::new();
            for iri in &attr_iris {
                let doc = serde_json::json!([{
                    "@id": "urn:ngsi-ld:_:dummy",
                    iri: [{"@value": "dummy"}]
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
                    Ok(compacted) => {
                        if let Some(obj) = compacted.as_object() {
                            obj.keys()
                                .find(|k| k.as_str() != "id" && k.as_str() != "type" && !k.starts_with('@'))
                                .cloned() // clone: owned String from borrowed map key
                                .unwrap_or_else(|| iri.clone()) // clone: fallback owned copy when no matching key found
                        } else {
                            iri.clone() // clone: fallback owned copy when compacted is not an object
                        }
                    }
                    Err(_) => iri.clone(), // clone: fallback owned copy when compaction fails
                };
                compacted_names.push(name);
            }
            compacted_names.sort();

            let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("AttributeList:{}", Uuid::new_v4())).build()?;
            let id = AttributeListId::try_from(id)?;

            let list = crate::domain::types::AttributeList {
                id,
                type_: "AttributeList".to_string(),
                attribute_list: compacted_names,
            };

            Ok(serde_json::to_value(list)?)
        }
    }

    /// Return detailed information for a single attribute, including its types and entity count.
    pub async fn attribute_info(&self, attr_id: &str, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let attr_iri = self.engine.expand_term(attr_id, context).await?;

        let row = self
            .repo
            .get_attribute_info(&attr_iri)
            .await?
            .ok_or_else(|| AndromedaError::entity_not_found(attr_id))?;

        let attr = crate::domain::types::Attribute {
            id: attr_iri.clone(), // clone: value also needed for attribute_name field below
            type_: "Attribute".to_string(),
            attribute_name: attr_iri,
            attribute_types: row.attribute_types,
            attribute_count: Some(row.entity_count),
            type_names: Some(row.entity_types),
        };

        let mut expanded_obj = attr.expand::<serde_json::Value>();
        if let Some(obj) = expanded_obj.as_object_mut() {
            obj.retain(|_, v| match v {
                Value::Array(arr) => !arr.is_empty(),
                _other => true,
            });
        }

        let expanded = serde_json::json!([expanded_obj]);
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;
        strip_ngsild_prefix(&mut compacted);
        ensure_array_fields(&mut compacted, &["attributeTypes", "typeNames"]);
        Ok(compacted)
    }
}

struct AttributeMetadata {
    entity_types: HashSet<String>,
}

/// Strip the `ngsi-ld:` prefix from compacted JSON keys.
///
/// The JSON-LD compactor sometimes produces CURIE forms like `ngsi-ld:attributeDetails`
/// instead of the bare short name `attributeDetails`, depending on how the context
/// defines the term (simple alias vs. expanded definition with `@container`).
/// The NGSI-LD API spec expects short names, so we normalize these in a post-pass.
fn strip_ngsild_prefix(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            let keys: Vec<String> = obj.keys().cloned().collect(); // clone: owned keys to avoid borrowing obj during mutation below
            for key in keys {
                if let Some(short) = key.strip_prefix("ngsi-ld:")
                    && let Some(val) = obj.remove(&key)
                {
                    obj.insert(short.to_string(), val);
                }
            }
            for val in obj.values_mut() {
                strip_ngsild_prefix(val);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                strip_ngsild_prefix(item);
            }
        }
        _other => {}
    }
}

/// Ensure specified fields are always arrays in compacted output.
/// JSON-LD compaction collapses single-element arrays to scalars, but the
/// NGSI-LD spec requires certain fields to always be arrays.
fn ensure_array_fields(value: &mut Value, fields: &[&str]) {
    match value {
        Value::Object(obj) => {
            for &field in fields {
                if let Some(val) = obj.get_mut(field)
                    && !val.is_array()
                {
                    *val = serde_json::json!([val]);
                }
            }
            // Recurse into nested arrays (attributeDetails, @graph, etc.)
            for val in obj.values_mut() {
                if val.is_array() || val.is_object() {
                    ensure_array_fields(val, fields);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                ensure_array_fields(item, fields);
            }
        }
        _other => {}
    }
}

#[inline]
fn extract_attribute_types(value: &Value) -> Option<Vec<String>> {
    let obj = match value {
        Value::Array(arr) => arr.first()?.as_object()?,
        Value::Object(obj) => obj,
        _other => return None,
    };

    obj.get("@type").and_then(|t| match t {
        Value::Array(arr) => {
            let types: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| crate::domain::attribute_type::AttributeType::from_iri_suffix(s).map(|t| t.to_string()))
                .collect();
            if types.is_empty() { None } else { Some(types) }
        }
        _other => None,
    })
}
