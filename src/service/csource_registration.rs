use crate::{
    core::error::AndromedaError,
    domain::{
        notification::{CsrChangeEvent, CsrTriggerReason},
        registration::ForwardingMode,
        temporal::{TimeProp, TimeRel},
    },
    http::parameter::query::{SharedQueryParams, csr::CsrQueryParameters},
    jsonld::{ContextSpec, JsonLdEngine},
    query::{
        compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter, IdPattern, Pagination},
        q::parser,
    },
    repository::csource_registration::{
        params::{CreateCsrParams, CsrUpdateFragment},
        repo::CsourceRegistrationRepository,
    },
    service::{csource_extract::unwrap_geo_property_to_geometry, csource_merge::apply_fragment},
    tenancy::types::TenantName,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use tokio::sync::broadcast;
use tracing::debug;

fn extract_create_params(payload: &Value) -> Result<CreateCsrParams<'_>, AndromedaError> {
    Ok(CreateCsrParams {
        registration_name: payload.get("registrationName").and_then(|v| v.as_str()),
        description: payload.get("description").and_then(|v| v.as_str()),
        information: payload.get("information"),
        tenant: payload.get("tenant").and_then(|v| v.as_str()),
        observation_interval: payload.get("observationInterval"),
        management_interval: payload.get("managementInterval"),
        location: payload
            .get("location")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        observation_space: payload
            .get("observationSpace")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        operation_space: payload
            .get("operationSpace")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        expires_at: payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .map(|s| {
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'expiresAt' value '{}': {}", s, e)))
            })
            .transpose()?,
        endpoint: payload.get("endpoint").and_then(|v| v.as_str()),
        accept: payload.get("accept").and_then(|v| v.as_str()),
        mode: payload
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<ForwardingMode>())
            .transpose()
            .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'mode' value: {}", e)))?
            .unwrap_or_default(),
        operations: payload
            .get("operations")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::csr_validation(format!("Invalid 'operations' value: {}", e)))) // clone: serde_json::from_value consumes the Value
            .transpose()?,
        scope: payload
            .get("scope")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::csr_validation(format!("Invalid 'scope' value: {}", e)))) // clone: serde_json::from_value consumes the Value
            .transpose()?,
        refresh_rate: payload.get("refreshRate").and_then(|v| v.as_str()),
        management: payload.get("management"),
        context_source_info: payload.get("contextSourceInfo"),
        jsonld_context: payload.get("jsonldContext").and_then(|v| v.as_str()),
        context_source_alias: payload.get("contextSourceAlias").and_then(|v| v.as_str()),
        additional_properties: extract_additional_properties(payload),
    })
}

fn extract_update_fragment(fragment: &Value) -> Result<CsrUpdateFragment, AndromedaError> {
    Ok(CsrUpdateFragment {
        registration_name: extract_optional_string(fragment, "registrationName"),
        description: extract_optional_string(fragment, "description"),
        information: fragment.get("information").cloned(), // clone: need owned Value for update fragment struct
        tenant: extract_optional_tenant_name(fragment, "tenant")?,
        observation_interval: extract_optional_value(fragment, "observationInterval"),
        management_interval: extract_optional_value(fragment, "managementInterval"),
        location: fragment
            .get("location")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        observation_space: fragment
            .get("observationSpace")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        operation_space: fragment
            .get("operationSpace")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        expires_at: fragment
            .get("expiresAt")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    v.as_str()
                        .ok_or_else(|| AndromedaError::csr_validation("'expiresAt' must be a string"))
                        .and_then(|s| {
                            DateTime::parse_from_rfc3339(s)
                                .map(|dt| Some(dt.with_timezone(&Utc)))
                                .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'expiresAt' value '{}': {}", s, e)))
                        })
                }
            })
            .transpose()?,
        endpoint: fragment.get("endpoint").and_then(|v| v.as_str()).map(|s| s.to_string()),
        accept: extract_optional_string(fragment, "accept"),
        mode: fragment
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<ForwardingMode>())
            .transpose()
            .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'mode' value: {}", e)))?,
        operations: fragment
            .get("operations")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone()) // clone: serde_json::from_value consumes the Value
                        .map(Some)
                        .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'operations' value: {}", e)))
                }
            })
            .transpose()?,
        scope: fragment
            .get("scope")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone()) // clone: serde_json::from_value consumes the Value
                        .map(Some)
                        .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'scope' value: {}", e)))
                }
            })
            .transpose()?,
        refresh_rate: extract_optional_string(fragment, "refreshRate"),
        management: extract_optional_value(fragment, "management"),
        context_source_info: extract_optional_value(fragment, "contextSourceInfo"),
        jsonld_context: extract_optional_string(fragment, "jsonldContext"),
        context_source_alias: extract_optional_string(fragment, "contextSourceAlias"),
        additional_properties: extract_additional_properties(fragment),
    })
}

/// For nullable string fields: absent -> None, null -> Some(None), value -> Some(Some(s))
fn extract_optional_string(payload: &Value, key: &str) -> Option<Option<String>> {
    payload.get(key).map(|v| if v.is_null() { None } else { v.as_str().map(|s| s.to_string()) })
}

/// For nullable Value fields: absent -> None, null -> Some(None), value -> Some(Some(v))
fn extract_optional_value(payload: &Value, key: &str) -> Option<Option<Value>> {
    payload.get(key).map(|v| if v.is_null() { None } else { Some(v.clone()) }) // clone: need owned Value; v is borrowed from payload
}

fn extract_optional_tenant_name(payload: &Value, key: &str) -> Result<Option<Option<TenantName>>, AndromedaError> {
    let value = payload.get(key);
    let result = match value {
        None => None,
        Some(raw) if raw.is_null() => Some(None),
        Some(raw) => {
            let tenant = raw
                .as_str()
                .ok_or_else(|| AndromedaError::csr_validation(format!("'{key}' must be a string")))?;
            let tenant_name =
                TenantName::try_from(tenant.to_string()).map_err(|_| AndromedaError::csr_validation(format!("'{key}' must be a non-empty string")))?;
            Some(Some(tenant_name))
        }
    };
    Ok(result)
}

fn extract_additional_properties(payload: &Value) -> Option<Value> {
    let obj = payload.as_object()?;
    let mut additional = serde_json::Map::new();
    for (key, value) in obj {
        if !CreateCsrParams::STANDARD_KEYS.contains(&key.as_str()) {
            additional.insert(key.clone(), value.clone()); // clone: owned key-value pairs for new additional properties Map
        }
    }
    if additional.is_empty() { None } else { Some(Value::Object(additional)) }
}

/// Service for managing NGSI-LD context source registrations.
pub struct CsourceRegistrationService {
    repo: CsourceRegistrationRepository,
    engine: JsonLdEngine,
    csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>,
}

impl CsourceRegistrationService {
    /// Create a new context source registration service backed by the given database pool.
    pub fn new(pool: &PgPool, engine: JsonLdEngine, csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>) -> CsourceRegistrationService {
        let repo = CsourceRegistrationRepository::new(pool);
        debug!("Context source registration service created!");
        CsourceRegistrationService { repo, engine, csr_change_tx }
    }

    fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref tx) = self.csr_change_tx {
            let _ = tx.send(event);
        }
    }

    fn is_valid_uri(id: &str) -> bool {
        crate::jsonld::term_resolver::is_valid_uri(id)
    }

    /// Create a new context source registration from the given JSON payload.
    pub async fn create(&self, payload: Value, context: &ContextSpec) -> Result<String, AndromedaError> {
        // Validate type
        if let Some(type_val) = payload.get("type")
            && type_val.as_str() != Some("ContextSourceRegistration")
        {
            return Err(AndromedaError::csr_validation("type must be 'ContextSourceRegistration'"));
        }

        // information is required
        if payload.get("information").is_none() {
            return Err(AndromedaError::csr_validation("information is required"));
        }

        // endpoint is required
        if payload.get("endpoint").is_none() {
            return Err(AndromedaError::csr_validation("endpoint is required"));
        }

        // Validate expiresAt is not in the past
        if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val) {
                if expires_at <= Utc::now() {
                    return Err(AndromedaError::csr_validation("expiresAt must be in the future"));
                }
            } else {
                return Err(AndromedaError::csr_validation("expiresAt must be a valid ISO 8601 date-time"));
            }
        }

        // Validate mode if present
        if let Some(mode) = payload.get("mode").and_then(|v| v.as_str())
            && !["inclusive", "exclusive", "redirect", "auxiliary"].contains(&mode)
        {
            return Err(AndromedaError::csr_validation("mode must be one of: inclusive, exclusive, redirect, auxiliary"));
        }

        // Validate id is a valid URI if provided
        if let Some(id) = payload.get("id").and_then(|v| v.as_str())
            && !Self::is_valid_uri(id)
        {
            return Err(AndromedaError::csr_validation("id must be a valid URI"));
        }

        // Determine the id
        let id = match payload.get("id").and_then(|v| v.as_str()) {
            Some(provided_id) => {
                if self.repo.exists(provided_id).await? {
                    return Err(AndromedaError::csr_already_exists(provided_id));
                }
                provided_id.to_string()
            }
            None => CsourceRegistrationRepository::generate_id()?,
        };

        // Validate that remote @context is reachable by expanding a minimal document
        let validation_doc = serde_json::json!({
            "id": "urn:ngsi-ld:_:context-validate",
            "type": "ContextSourceRegistration"
        });
        self.engine.expand(validation_doc, context).await?;

        // Expand entity types and attribute names in information
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        let params = extract_create_params(&payload)?;
        self.repo.create(&id, &params).await?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(), // clone: id is also returned via Ok(id) below
            trigger_reason: CsrTriggerReason::NewlyMatching,
            csr_snapshot: None,
        });

        Ok(id)
    }

    /// Retrieve a single context source registration by its identifier.
    pub async fn get(&self, id: &str, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let csr = self.repo.get(id).await?.ok_or_else(|| AndromedaError::csr_not_found(id))?;

        let mut value = serde_json::to_value(&csr).map_err(|e| AndromedaError::internal(format!("Failed to serialize CSR: {}", e)))?;

        // Compact entity types and attribute names in information
        if let Some(information) = value.get_mut("information") {
            self.compact_information(information, context).await?;
        }

        Ok(value)
    }

    /// Query context source registrations matching the given parameters.
    pub async fn query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<(Vec<Value>, i64), AndromedaError> {
        params.validate().map_err(|e| AndromedaError::csr_validation(e.to_string()))?;

        let compiled = self.compile_csr_query(params, context).await?;

        let (csrs, total) = self.repo.query_compiled(&compiled).await?;

        let mut values = Vec::with_capacity(csrs.len());
        for csr in csrs {
            let mut value = serde_json::to_value(&csr).map_err(|e| AndromedaError::internal(format!("Failed to serialize CSR: {}", e)))?;
            if let Some(information) = value.get_mut("information") {
                self.compact_information(information, context).await?;
            }
            values.push(value);
        }

        Ok((values, total))
    }

    async fn compile_csr_query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<CompiledCsrQuery, AndromedaError> {
        // Expand type
        let expanded_type = if let Some(ref t) = params.entity_type {
            Some(self.engine.expand_type(t, context).await?)
        } else {
            None
        };

        // Expand attrs
        let raw_attrs = params.parse_attrs();
        let expanded_attrs = if !raw_attrs.is_empty() {
            Some(crate::query::compile::expand_attrs(&raw_attrs, &self.engine, context).await?)
        } else {
            None
        };

        // Parse geo filter
        let geo = crate::query::compile::geo::compile_geo_query(
            params.georel.as_deref(),
            params.geometry.as_deref(),
            params.coordinates.as_deref(),
            None,
            &self.engine,
            context,
        )
        .await?;

        // Parse temporal filter
        let temporal = if let (Some(timerel), Some(time_at)) = (&params.timerel, &params.time_at) {
            let parsed_time_at = chrono::DateTime::parse_from_rfc3339(time_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|_| AndromedaError::csr_validation("Invalid timeAt: must be ISO 8601 date-time"))?;
            let parsed_end_time_at = params
                .end_time_at
                .as_deref()
                .map(|s| {
                    chrono::DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .map_err(|_| AndromedaError::csr_validation("Invalid endTimeAt: must be ISO 8601 date-time"))
                })
                .transpose()?;
            let parsed_timerel = timerel
                .parse::<TimeRel>()
                .map_err(|e| AndromedaError::csr_validation(format!("Invalid timerel: {}", e)))?;
            let parsed_timeprop = params
                .timeproperty
                .as_deref()
                .map(|s| s.parse::<TimeProp>())
                .transpose()
                .map_err(|e| AndromedaError::csr_validation(format!("Invalid timeproperty: {}", e)))?;
            Some(CompiledCsrTemporalFilter {
                timerel: parsed_timerel,
                time_at: parsed_time_at,
                end_time_at: parsed_end_time_at,
                timeproperty: parsed_timeprop,
            })
        } else {
            None
        };

        // Parse q expression
        let q_expr = if let Some(ref q) = params.q {
            if !q.is_empty() {
                Some(parser::parse(q).map_err(|e| AndromedaError::csr_validation(format!("Invalid q parameter: {}", e)))?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(CompiledCsrQuery {
            ids: params.parse_ids(),
            entity_type: expanded_type,
            id_pattern: params
                .id_pattern
                .as_deref()
                .map(IdPattern::new)
                .transpose()
                .map_err(|e| AndromedaError::csr_validation(e.to_string()))?,
            attrs: expanded_attrs,
            q_expr,
            geo,
            temporal,
            pagination: Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: false,
            },
        })
    }

    /// Update an existing context source registration with the given fragment.
    pub async fn update(&self, id: &str, payload: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if !Self::is_valid_uri(id) {
            return Err(AndromedaError::csr_validation("Context source registration id must be present and a valid URI"));
        }

        // Validate type if present
        if let Some(type_val) = payload.get("type")
            && type_val.as_str() != Some("ContextSourceRegistration")
        {
            return Err(AndromedaError::csr_validation("type must be 'ContextSourceRegistration'"));
        }

        // Reject nulling out mandatory fields
        if payload.get("information").is_some_and(|v| v.is_null()) {
            return Err(AndromedaError::csr_validation("information must not be null"));
        }
        if payload.get("endpoint").is_some_and(|v| v.is_null()) {
            return Err(AndromedaError::csr_validation("endpoint must not be null"));
        }

        // Validate expiresAt if present
        if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val) {
                if expires_at <= Utc::now() {
                    return Err(AndromedaError::csr_validation("expiresAt must be in the future"));
                }
            } else {
                return Err(AndromedaError::csr_validation("expiresAt must be a valid ISO 8601 date-time"));
            }
        }

        // Validate mode if present
        if let Some(mode) = payload.get("mode").and_then(|v| v.as_str())
            && !["inclusive", "exclusive", "redirect", "auxiliary"].contains(&mode)
        {
            return Err(AndromedaError::csr_validation("mode must be one of: inclusive, exclusive, redirect, auxiliary"));
        }

        // Expand entity types and attribute names in information if present
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        // Take a pre-update snapshot so the notification worker can detect noLongerMatching
        let pre_update_snapshot = self.repo.get(id).await?.map(|csr| serde_json::to_value(&csr).unwrap_or(Value::Null));

        let fragment = extract_update_fragment(&payload)?;
        let existing = match self.repo.get(id).await? {
            Some(csr) => csr,
            None => return Err(AndromedaError::csr_not_found(id)),
        };
        let resolved = apply_fragment(existing, &fragment)?;
        let updated = self.repo.update(id, &resolved).await?;

        if updated == 0 {
            return Err(AndromedaError::csr_not_found(id));
        }

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.to_string(),
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: pre_update_snapshot,
        });

        Ok(())
    }

    /// Delete a context source registration by its identifier.
    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if !Self::is_valid_uri(id) {
            return Err(AndromedaError::csr_validation("Context source registration id must be present and a valid URI"));
        }

        // Fetch snapshot before deletion for notification
        let snapshot = self.repo.get(id).await?.map(|csr| serde_json::to_value(&csr).unwrap_or(Value::Null));

        let deleted = self.repo.delete(id).await?;

        if deleted == 0 {
            return Err(AndromedaError::csr_not_found(id));
        }

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.to_string(),
            trigger_reason: CsrTriggerReason::NoLongerMatching,
            csr_snapshot: snapshot,
        });

        Ok(())
    }

    /// Expand entity types and attribute names in the `information` array in-place.
    async fn expand_information(&self, information: &mut Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if let Some(info_arr) = information.as_array_mut() {
            for info in info_arr.iter_mut() {
                // Expand entity types
                if let Some(entities) = info.get_mut("entities")
                    && let Some(entities_arr) = entities.as_array_mut()
                {
                    for entity in entities_arr.iter_mut() {
                        if let Some(type_val) = entity.get("type").and_then(|v| v.as_str()) {
                            let expanded = self.engine.expand_type(type_val, context).await?;
                            if let Some(obj) = entity.as_object_mut() {
                                obj.insert("type".to_string(), Value::String(expanded));
                            }
                        }
                    }
                }

                // Expand property names
                if let Some(prop_names) = info.get("propertyNames")
                    && let Some(names_arr) = prop_names.as_array()
                {
                    let mut expanded_names = Vec::new();
                    for name in names_arr {
                        if let Some(n) = name.as_str() {
                            expanded_names.push(Value::String(self.engine.expand_term(n, context).await?));
                        }
                    }
                    if let Some(obj) = info.as_object_mut() {
                        obj.insert("propertyNames".to_string(), Value::Array(expanded_names));
                    }
                }

                // Expand relationship names
                if let Some(rel_names) = info.get("relationshipNames")
                    && let Some(names_arr) = rel_names.as_array()
                {
                    let mut expanded_names = Vec::new();
                    for name in names_arr {
                        if let Some(n) = name.as_str() {
                            expanded_names.push(Value::String(self.engine.expand_term(n, context).await?));
                        }
                    }
                    if let Some(obj) = info.as_object_mut() {
                        obj.insert("relationshipNames".to_string(), Value::Array(expanded_names));
                    }
                }
            }
        }
        Ok(())
    }

    /// Compact entity types and attribute names in the `information` array in-place.
    async fn compact_information(&self, information: &mut Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if let Some(info_arr) = information.as_array_mut() {
            for info in info_arr.iter_mut() {
                // Compact entity types
                if let Some(entities) = info.get_mut("entities")
                    && let Some(entities_arr) = entities.as_array_mut()
                {
                    for entity in entities_arr.iter_mut() {
                        if let Some(type_val) = entity.get("type").and_then(|v| v.as_str()) {
                            let compacted = crate::jsonld::term_resolver::compact_type_iri(&self.engine, type_val, context).await;
                            if let Some(obj) = entity.as_object_mut() {
                                obj.insert("type".to_string(), Value::String(compacted));
                            }
                        }
                    }
                }

                // Compact property names
                if let Some(prop_names) = info.get("propertyNames")
                    && let Some(names_arr) = prop_names.as_array()
                {
                    let mut compacted_names = Vec::new();
                    for name in names_arr {
                        if let Some(n) = name.as_str() {
                            compacted_names.push(Value::String(crate::jsonld::term_resolver::compact_attr_iri(&self.engine, n, context).await));
                        }
                    }
                    if let Some(obj) = info.as_object_mut() {
                        obj.insert("propertyNames".to_string(), Value::Array(compacted_names));
                    }
                }

                // Compact relationship names
                if let Some(rel_names) = info.get("relationshipNames")
                    && let Some(names_arr) = rel_names.as_array()
                {
                    let mut compacted_names = Vec::new();
                    for name in names_arr {
                        if let Some(n) = name.as_str() {
                            compacted_names.push(Value::String(crate::jsonld::term_resolver::compact_attr_iri(&self.engine, n, context).await));
                        }
                    }
                    if let Some(obj) = info.as_object_mut() {
                        obj.insert("relationshipNames".to_string(), Value::Array(compacted_names));
                    }
                }
            }
        }
        Ok(())
    }
}
