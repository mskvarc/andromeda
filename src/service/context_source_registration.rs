use crate::{
    domain::notification::{CsrChangeEvent, CsrTriggerReason},
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    query::{
        compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
        csr::CsrQueryParameters,
        params::SharedQueryParams,
    },
    repository::context_source_registration::{ContextSourceRegistrationRepository, CreateCsrParams, CsrUpdateFragment, unwrap_geo_property_to_string},
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
        location: payload.get("location"),
        observation_space: payload.get("observationSpace"),
        operation_space: payload.get("operationSpace"),
        expires_at: payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok()),
        endpoint: payload.get("endpoint").and_then(|v| v.as_str()),
        accept: payload.get("accept").and_then(|v| v.as_str()),
        mode: payload.get("mode").and_then(|v| v.as_str()).unwrap_or("inclusive"),
        operations: payload
            .get("operations")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::bad_request_data(format!("Invalid 'operations' value: {}", e))))
            .transpose()?,
        scope: payload
            .get("scope")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| AndromedaError::bad_request_data(format!("Invalid 'scope' value: {}", e))))
            .transpose()?,
        refresh_rate: payload.get("refreshRate").and_then(|v| v.as_str()),
        management: payload.get("management"),
        context_source_info: payload.get("contextSourceInfo"),
        jsonld_context: payload.get("jsonldContext").and_then(|v| v.as_str()),
        additional_properties: extract_additional_properties(payload),
    })
}

fn extract_update_fragment(fragment: &Value) -> Result<CsrUpdateFragment, AndromedaError> {
    Ok(CsrUpdateFragment {
        registration_name: extract_optional_string(fragment, "registrationName"),
        description: extract_optional_string(fragment, "description"),
        information: fragment.get("information").cloned(),
        tenant: extract_optional_string(fragment, "tenant"),
        observation_interval: extract_optional_value(fragment, "observationInterval"),
        management_interval: extract_optional_value(fragment, "managementInterval"),
        location: fragment
            .get("location")
            .map(|v| if v.is_null() { None } else { Some(unwrap_geo_property_to_string(v)) }),
        observation_space: fragment
            .get("observationSpace")
            .map(|v| if v.is_null() { None } else { Some(unwrap_geo_property_to_string(v)) }),
        operation_space: fragment
            .get("operationSpace")
            .map(|v| if v.is_null() { None } else { Some(unwrap_geo_property_to_string(v)) }),
        expires_at: fragment.get("expiresAt").map(|v| {
            if v.is_null() {
                None
            } else {
                v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok())
            }
        }),
        endpoint: fragment.get("endpoint").and_then(|v| v.as_str()).map(|s| s.to_string()),
        accept: extract_optional_string(fragment, "accept"),
        mode: fragment.get("mode").and_then(|v| v.as_str()).map(|s| s.to_string()),
        operations: fragment
            .get("operations")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone())
                        .map(Some)
                        .map_err(|e| AndromedaError::bad_request_data(format!("Invalid 'operations' value: {}", e)))
                }
            })
            .transpose()?,
        scope: fragment
            .get("scope")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone())
                        .map(Some)
                        .map_err(|e| AndromedaError::bad_request_data(format!("Invalid 'scope' value: {}", e)))
                }
            })
            .transpose()?,
        refresh_rate: extract_optional_string(fragment, "refreshRate"),
        management: extract_optional_value(fragment, "management"),
        context_source_info: extract_optional_value(fragment, "contextSourceInfo"),
        jsonld_context: extract_optional_string(fragment, "jsonldContext"),
        additional_properties: extract_additional_properties(fragment),
    })
}

/// For nullable string fields: absent -> None, null -> Some(None), value -> Some(Some(s))
fn extract_optional_string(payload: &Value, key: &str) -> Option<Option<String>> {
    payload.get(key).map(|v| if v.is_null() { None } else { v.as_str().map(|s| s.to_string()) })
}

/// For nullable Value fields: absent -> None, null -> Some(None), value -> Some(Some(v))
fn extract_optional_value(payload: &Value, key: &str) -> Option<Option<Value>> {
    payload.get(key).map(|v| if v.is_null() { None } else { Some(v.clone()) })
}

fn extract_additional_properties(payload: &Value) -> Option<Value> {
    let obj = payload.as_object()?;
    let mut additional = serde_json::Map::new();
    for (key, value) in obj {
        if !CreateCsrParams::STANDARD_KEYS.contains(&key.as_str()) {
            additional.insert(key.clone(), value.clone());
        }
    }
    if additional.is_empty() { None } else { Some(Value::Object(additional)) }
}

pub struct ContextSourceRegistrationService {
    repo: ContextSourceRegistrationRepository,
    engine: JsonLdEngine,
    csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>,
}

impl ContextSourceRegistrationService {
    pub fn new(pool: &PgPool, engine: JsonLdEngine, csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>) -> Self {
        let repo = ContextSourceRegistrationRepository::new(pool);
        debug!("Context source registration service created!");
        Self { repo, engine, csr_change_tx }
    }

    fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref tx) = self.csr_change_tx {
            let _ = tx.send(event);
        }
    }

    fn is_valid_uri(id: &str) -> bool {
        crate::jsonld::term_resolver::is_valid_uri(id)
    }

    pub async fn create(&self, payload: Value, context: &ContextSpec) -> Result<String, AndromedaError> {
        // Validate type
        if let Some(type_val) = payload.get("type")
            && type_val.as_str() != Some("ContextSourceRegistration")
        {
            return Err(AndromedaError::bad_request_data("type must be 'ContextSourceRegistration'"));
        }

        // information is required
        if payload.get("information").is_none() {
            return Err(AndromedaError::bad_request_data("information is required"));
        }

        // endpoint is required
        if payload.get("endpoint").is_none() {
            return Err(AndromedaError::bad_request_data("endpoint is required"));
        }

        // Validate expiresAt is not in the past
        if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val) {
                if expires_at <= Utc::now() {
                    return Err(AndromedaError::bad_request_data("expiresAt must be in the future"));
                }
            } else {
                return Err(AndromedaError::bad_request_data("expiresAt must be a valid ISO 8601 date-time"));
            }
        }

        // Validate mode if present
        if let Some(mode) = payload.get("mode").and_then(|v| v.as_str())
            && !["inclusive", "exclusive", "redirect", "auxiliary"].contains(&mode)
        {
            return Err(AndromedaError::bad_request_data(
                "mode must be one of: inclusive, exclusive, redirect, auxiliary",
            ));
        }

        // Validate id is a valid URI if provided
        if let Some(id) = payload.get("id").and_then(|v| v.as_str())
            && !Self::is_valid_uri(id)
        {
            return Err(AndromedaError::bad_request_data("id must be a valid URI"));
        }

        // Determine the id
        let id = match payload.get("id").and_then(|v| v.as_str()) {
            Some(provided_id) => {
                if self.repo.exists(provided_id).await? {
                    return Err(AndromedaError::already_exists(provided_id));
                }
                provided_id.to_string()
            }
            None => ContextSourceRegistrationRepository::generate_id()?,
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
            csr_id: id.clone(),
            trigger_reason: CsrTriggerReason::NewlyMatching,
            csr_snapshot: None,
        });

        Ok(id)
    }

    pub async fn get(&self, id: &str, context: &ContextSpec) -> Result<Value, AndromedaError> {
        let csr = self.repo.get(id).await?.ok_or_else(|| AndromedaError::not_found(id))?;

        let mut value = serde_json::to_value(&csr).map_err(|e| AndromedaError::internal(format!("Failed to serialize CSR: {}", e)))?;

        // Compact entity types and attribute names in information
        if let Some(information) = value.get_mut("information") {
            self.compact_information(information, context).await?;
        }

        Ok(value)
    }

    pub async fn query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<(Vec<Value>, i64), AndromedaError> {
        params.validate().map_err(|e| AndromedaError::bad_request_data(e.to_string()))?;

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
        let geo = crate::query::geo::compile_geo_query(
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
            // Validate time_at is a valid ISO 8601 datetime
            chrono::DateTime::parse_from_rfc3339(time_at).map_err(|_| AndromedaError::bad_request_data("Invalid timeAt: must be ISO 8601 date-time"))?;
            if let Some(ref end) = params.end_time_at {
                chrono::DateTime::parse_from_rfc3339(end).map_err(|_| AndromedaError::bad_request_data("Invalid endTimeAt: must be ISO 8601 date-time"))?;
            }
            Some(CompiledCsrTemporalFilter {
                timerel: timerel.clone(),
                time_at: time_at.clone(),
                end_time_at: params.end_time_at.clone(),
                timeproperty: params.timeproperty.clone(),
            })
        } else {
            None
        };

        // Parse q expression
        let q_expr = if let Some(ref q) = params.q {
            if !q.is_empty() {
                Some(crate::query::q::parse(q).map_err(|e| AndromedaError::bad_request_data(format!("Invalid q parameter: {}", e)))?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(CompiledCsrQuery {
            ids: params.parse_ids(),
            entity_type: expanded_type,
            id_pattern: params.id_pattern.clone(),
            attrs: expanded_attrs,
            q_expr,
            geo,
            temporal,
            limit: params.effective_limit(),
            offset: params.effective_offset(),
        })
    }

    pub async fn update(&self, id: &str, payload: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if !Self::is_valid_uri(id) {
            return Err(AndromedaError::bad_request_data(
                "Context source registration id must be present and a valid URI",
            ));
        }

        // Validate type if present
        if let Some(type_val) = payload.get("type")
            && type_val.as_str() != Some("ContextSourceRegistration")
        {
            return Err(AndromedaError::bad_request_data("type must be 'ContextSourceRegistration'"));
        }

        // Reject nulling out mandatory fields
        if payload.get("information").is_some_and(|v| v.is_null()) {
            return Err(AndromedaError::bad_request_data("information must not be null"));
        }
        if payload.get("endpoint").is_some_and(|v| v.is_null()) {
            return Err(AndromedaError::bad_request_data("endpoint must not be null"));
        }

        // Validate expiresAt if present
        if let Some(expires_at_val) = payload.get("expiresAt").and_then(|v| v.as_str()) {
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_val) {
                if expires_at <= Utc::now() {
                    return Err(AndromedaError::bad_request_data("expiresAt must be in the future"));
                }
            } else {
                return Err(AndromedaError::bad_request_data("expiresAt must be a valid ISO 8601 date-time"));
            }
        }

        // Validate mode if present
        if let Some(mode) = payload.get("mode").and_then(|v| v.as_str())
            && !["inclusive", "exclusive", "redirect", "auxiliary"].contains(&mode)
        {
            return Err(AndromedaError::bad_request_data(
                "mode must be one of: inclusive, exclusive, redirect, auxiliary",
            ));
        }

        // Expand entity types and attribute names in information if present
        let mut payload = payload;
        if let Some(information) = payload.get_mut("information") {
            self.expand_information(information, context).await?;
        }

        // Take a pre-update snapshot so the notification worker can detect noLongerMatching
        let pre_update_snapshot = self.repo.get(id).await?.map(|csr| serde_json::to_value(&csr).unwrap_or(Value::Null));

        let fragment = extract_update_fragment(&payload)?;
        let updated = self.repo.update(id, &fragment).await?;

        if !updated {
            return Err(AndromedaError::not_found(id));
        }

        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.to_string(),
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: pre_update_snapshot,
        });

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if !Self::is_valid_uri(id) {
            return Err(AndromedaError::bad_request_data(
                "Context source registration id must be present and a valid URI",
            ));
        }

        // Fetch snapshot before deletion for notification
        let snapshot = self.repo.get(id).await?.map(|csr| serde_json::to_value(&csr).unwrap_or(Value::Null));

        let deleted = self.repo.delete(id).await?;

        if !deleted {
            return Err(AndromedaError::not_found(id));
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
