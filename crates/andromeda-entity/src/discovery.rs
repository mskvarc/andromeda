use crate::{attribute_summary::AttributeSummary, error::EntityError, port::discovery::EntityDiscoveryStore};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    ngsi_ld_urn::{AttributeListId, EntityTypeListId},
    uri::NGSI_LD_URN_NID,
};
use andromeda_ngsi_ld_model::{
    attribute_list::{AttributeDetail, AttributeInfo, AttributeList, AttributeListType, AttributeResourceType},
    entity_type_list::{EntityType, EntityTypeInfo, EntityTypeInfoType, EntityTypeList, EntityTypeListType, EntityTypeType},
};
use iri_rs::IriBuf;
use jsonld::Expandable;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Service for NGSI-LD entity type and attribute discovery operations.
pub struct EntityDiscoveryService {
    store: Arc<dyn EntityDiscoveryStore>,
    engine: JsonLdEngine,
}

/// Representation requested for an attribute collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeCollectionRepresentation {
    /// Return identifiers only.
    List,
    /// Return the detailed resource representation.
    Detailed,
}

impl EntityDiscoveryService {
    /// Create a new entity discovery service backed by the given store.
    pub fn new(store: Arc<dyn EntityDiscoveryStore>, engine: JsonLdEngine) -> EntityDiscoveryService {
        EntityDiscoveryService { store, engine }
    }

    /// List all known entity types as a compacted `EntityTypeList`.
    pub async fn types(&self, context: &ContextSpec) -> Result<EntityTypeList, EntityError> {
        const TYPE: &str = "EntityTypeList";

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("{}:{}", TYPE, Uuid::new_v4())).build()?;
        let id = EntityTypeListId::try_from(id)?;

        let type_iris = self.store.types().await?;

        // Compact each type IRI using a synthetic doc
        let mut type_list = Vec::new();
        for iri in &type_iris {
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
            type_list.push(parse_type_name(name)?);
        }
        type_list.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        let result = EntityTypeList {
            id,
            type_: EntityTypeListType,
            type_list,
        };

        Ok(result)
    }

    /// Return detailed information for all entity types, including their attribute names.
    pub async fn type_details(&self, context: &ContextSpec) -> Result<Value, EntityError> {
        // Keyset-paginate the aggregation so a very large set of distinct
        // types does not require a single large result set.
        const PAGE: u32 = 500;
        let mut summaries: Vec<(EntityTypeName, Vec<AttributeName>)> = Vec::new();
        let mut cursor: Option<EntityTypeName> = None;
        loop {
            let page = self.store.get_entity_type_summaries(cursor.as_ref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|(t, _)| t.clone()); // clone: own cursor for next loop iteration
            let page_len = page.len();
            summaries.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }

        let entity_types: Vec<EntityType> = summaries
            .into_iter()
            .map(|(type_iri, attrs)| {
                Ok(EntityType {
                    id: parse_discovery_iri(type_iri.to_string())?,
                    type_name: type_iri,
                    type_: EntityTypeType,
                    attribute_names: attrs,
                })
            })
            .collect::<Result<_, EntityError>>()?;

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
    pub async fn type_info(&self, type_name: &EntityTypeName, context: &ContextSpec) -> Result<Value, EntityError> {
        // Expand the type name first — DB stores expanded IRIs
        let prepared_context = self.engine.prepare_term_context(context);
        let type_iri = self.engine.expand_type(type_name.as_str(), &prepared_context).await?;
        let type_key = parse_type_name(type_iri.as_str())?;

        let entities = self.store.get_entities_by_type(&type_key, None, None).await?;
        let count = self.store.count_by_type(&type_key).await?;

        if count == 0 {
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
                _other => None,
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

    /// List all known attributes, optionally including per-attribute detail.
    pub async fn attributes(&self, representation: AttributeCollectionRepresentation, context: &ContextSpec) -> Result<Value, EntityError> {
        // Keyset-paginate attribute summary aggregation.
        const PAGE: u32 = 500;
        let mut summary_rows: Vec<AttributeSummary> = Vec::new();
        let mut cursor: Option<AttributeName> = None;
        loop {
            let page = self.store.get_attribute_summary(cursor.as_ref(), PAGE).await?;
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
        for row in &summary_rows {
            attr_data.insert(
                row.attr_iri.clone(), // clone: owned key for HashMap entry
                AttributeMetadata {
                    entity_types: row.entity_types.iter().cloned().collect(), // clone: owned names from borrowed Vec
                },
            );
        }

        if matches!(representation, AttributeCollectionRepresentation::Detailed) {
            // details=true on /attributes returns a simplified view:
            // id, type, attributeName, typeNames (no attributeTypes/attributeCount)
            let mut attributes = Vec::new();
            for (iri, meta) in attr_data {
                attributes.push(
                    AttributeInfo::builder()
                        .attribute_name(iri.clone()) // clone: the record owns its name and its id is derived from the same value
                        .id(parse_discovery_iri(iri.to_string())?)
                        .type_(AttributeResourceType)
                        .attribute_types(Vec::new())
                        .attribute_count(None)
                        .type_names(Some(meta.entity_types.into_iter().collect()))
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
            let attr_iris: Vec<AttributeName> = attr_data.into_keys().collect();

            // Compact each attribute IRI using a synthetic expanded doc
            let mut compacted_names: Vec<AttributeName> = Vec::new();
            for iri in &attr_iris {
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
                    Ok(compacted) => {
                        if let Some(obj) = compacted.as_object() {
                            obj.keys()
                                .find(|k| k.as_str() != "id" && k.as_str() != "type" && !k.starts_with('@'))
                                .cloned() // clone: owned String from borrowed map key
                                .unwrap_or_else(|| iri.to_string()) // fallback to the IRI itself when no matching key found
                        } else {
                            iri.to_string() // fallback to the IRI itself when compacted is not an object
                        }
                    }
                    Err(_) => iri.to_string(), // fallback to the IRI itself when compaction fails
                };
                compacted_names.push(parse_attribute_name(&name)?);
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
    }

    /// Return detailed information for a single attribute, including its types and entity count.
    pub async fn attribute_info(&self, attr_id: &AttributeName, context: &ContextSpec) -> Result<Value, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri_buf = self.engine.expand_term(attr_id.as_str(), &prepared_context).await?;
        let attr_iri = parse_attribute_name(attr_iri_buf.as_str())?;

        let row = self
            .store
            .get_attribute_info(&attr_iri)
            .await?
            .ok_or_else(|| EntityError::attribute_not_found(attr_id))?;

        // Clause 5.2.28 fixes `attributeTypes` to the attribute family names; the
        // store already mapped the payload's expanded `@type` IRIs to them.
        let attr = AttributeInfo::builder()
            .attribute_name(attr_iri)
            .id(attr_iri_buf)
            .attribute_types(row.attribute_types)
            .attribute_count(Some(row.entity_count))
            .type_names(Some(row.entity_types))
            .build();

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
    entity_types: HashSet<EntityTypeName>,
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
/// JSON-LD compaction collapses single-element arrays to scalars, but several
/// NGSI-LD discovery members are defined as `String[]` (e.g. `attributeTypes`,
/// `typeNames`; clauses 5.2.24 to 5.2.28) and must stay arrays.
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

/// Parse a stored entity-type name, reporting a malformed one as an internal
/// fault: the value came out of the broker's own store or its own compactor.
fn parse_type_name(value: &str) -> Result<EntityTypeName, EntityError> {
    EntityTypeName::from_str(value).map_err(|error| EntityError::internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute name, reporting a malformed one as an internal fault
/// for the same reason as [`parse_type_name`].
fn parse_attribute_name(value: &str) -> Result<AttributeName, EntityError> {
    AttributeName::from_str(value).map_err(|error| EntityError::internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored expanded IRI, reporting a malformed one as an internal fault
/// for the same reason as [`parse_type_name`]: the broker wrote it.
fn parse_discovery_iri(value: String) -> Result<IriBuf, EntityError> {
    IriBuf::new(value).map_err(|error| EntityError::internal(format!("stored discovery identifier is not a valid IRI: {error}")))
}

#[inline]
fn extract_attribute_types(value: &Value) -> Option<Vec<AttributeType>> {
    let obj = match value {
        Value::Array(arr) => arr.first()?.as_object()?,
        Value::Object(obj) => obj,
        _other => return None,
    };

    obj.get("@type").and_then(|t| match t {
        Value::Array(arr) => {
            let types: Vec<AttributeType> = arr.iter().filter_map(|v| v.as_str()).filter_map(AttributeType::from_iri_suffix).collect();
            if types.is_empty() { None } else { Some(types) }
        }
        _other => None,
    })
}
