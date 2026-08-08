use crate::{
    error::AndromedaError,
    event::csr_change_event::CsrChangeEvent,
    service::{
        csource_extract::unwrap_geo_property_to_geometry,
        csource_merge::apply_fragment,
        csource_registration::{
            payload::{extract_create_params, extract_update_fragment},
            store::{CreateCsrParams, CsrUpdateFragment, RegistrationStore},
        },
    },
};
use andromeda_jsonld::{
    compaction::{compact_attr_iri, compact_type_iri},
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::{
    tenant::TenantName,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ForwardingMode};
use andromeda_ngsi_ld_query::{
    csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
    geo::compile as geo_compile,
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::window::TimeInterval,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, csr::CsrQueryParameters};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;
use urn_rs::UrnBuilder;
use uuid::Uuid;

mod payload;
/// The registration store port, and the parameter shapes it takes.
pub mod store;

/// Service for managing NGSI-LD context source registrations.
pub struct CsourceRegistrationService {
    repo: Arc<dyn RegistrationStore>,
    engine: JsonLdEngine,
    csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>,
}

impl CsourceRegistrationService {
    /// Create a new context source registration service backed by the given database pool.
    pub fn new(repo: Arc<dyn RegistrationStore>, engine: JsonLdEngine, csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>) -> CsourceRegistrationService {
        debug!("Context source registration service created!");
        CsourceRegistrationService { repo, engine, csr_change_tx }
    }

    /// Mint a broker-assigned registration identifier (clause 5.10.2).
    fn generate_registration_id() -> Result<String, AndromedaError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
        Ok(urn.to_string())
    }

    fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref tx) = self.csr_change_tx {
            let _ = tx.send(event);
        }
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
            && id.parse::<Uri>().is_err()
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
            None => Self::generate_registration_id()?,
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
        let csr_uri: Uri = id
            .parse()
            .map_err(|_| AndromedaError::csr_validation("Context source registration id must be a valid URI"))?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: csr_uri,
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
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);

        // Expand type
        let expanded_type = match params.entity_type.as_ref() {
            Some(entity_type) => match entity_type.as_single_type() {
                Some(entity_type) => Some(expander.expand_type(entity_type).await?),
                None => return Err(AndromedaError::csr_validation("CSR discovery requires a single entity type".to_string())),
            },
            None => None,
        };

        // Expand attrs
        let expanded_attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;

        // Parse geo filter
        let geo = geo_compile::compile_geo_query(params.geo.0.as_ref(), &expander).await?;

        // Parse temporal filter
        let temporal = if let (Some(timerel), Some(time_at)) = (&params.timerel, &params.time_at) {
            let parsed_time_at = *time_at;
            let parsed_end_time_at = params.end_time_at;
            // `timerel` and `timeproperty` were validated as their enums at the
            // query boundary.
            let parsed_timerel = *timerel;
            let parsed_timeprop = params.timeproperty;
            Some(
                CompiledCsrTemporalFilter::builder()
                    .interval(TimeInterval::from_parts(Some(parsed_timerel), Some(parsed_time_at), parsed_end_time_at)?)
                    .timeproperty(parsed_timeprop)
                    .build(),
            )
        } else {
            None
        };

        // The query extractor has already parsed clause 4.9 syntax.
        let q_expr = params.q.clone(); // clone: compiled output owns its expression while parameters remain borrowed.

        Ok(CompiledCsrQuery::builder()
            .ids(params.id.clone().unwrap_or_default())
            .entity_type(expanded_type)
            .id_pattern(params.id_pattern.clone())
            .attrs(expanded_attrs)
            .q_expr(q_expr)
            .geo(geo)
            .temporal(temporal)
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                // Clause 5.10.2 discovery has no count option of its own.
                count: CountPolicy::Omit,
            })
            .build())
    }

    /// Update an existing context source registration with the given fragment.
    pub async fn update(&self, id: &str, payload: Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        if id.parse::<Uri>().is_err() {
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
        let csr_uri: Uri = id
            .parse()
            .map_err(|_| AndromedaError::csr_validation("Context source registration id must be a valid URI"))?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: csr_uri,
            trigger_reason: CsrTriggerReason::Updated,
            csr_snapshot: pre_update_snapshot,
        });

        Ok(())
    }

    /// Delete a context source registration by its identifier.
    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if id.parse::<Uri>().is_err() {
            return Err(AndromedaError::csr_validation("Context source registration id must be present and a valid URI"));
        }

        // Fetch snapshot before deletion for notification
        let snapshot = self.repo.get(id).await?.map(|csr| serde_json::to_value(&csr).unwrap_or(Value::Null));

        let deleted = self.repo.delete(id).await?;

        if deleted == 0 {
            return Err(AndromedaError::csr_not_found(id));
        }
        let csr_uri: Uri = id
            .parse()
            .map_err(|_| AndromedaError::csr_validation("Context source registration id must be a valid URI"))?;

        self.emit_csr_change(CsrChangeEvent {
            csr_id: csr_uri,
            trigger_reason: CsrTriggerReason::NoLongerMatching,
            csr_snapshot: snapshot,
        });

        Ok(())
    }

    /// Expand entity types and attribute names in the `information` array in-place.
    async fn expand_information(&self, information: &mut Value, context: &ContextSpec) -> Result<(), AndromedaError> {
        let prepared_context = self.engine.prepare_term_context(context);
        if let Some(info_arr) = information.as_array_mut() {
            for info in info_arr.iter_mut() {
                // Expand entity types
                if let Some(entities) = info.get_mut("entities")
                    && let Some(entities_arr) = entities.as_array_mut()
                {
                    for entity in entities_arr.iter_mut() {
                        if let Some(type_val) = entity.get("type").and_then(|v| v.as_str()) {
                            let expanded = self.engine.expand_type(type_val, &prepared_context).await?.to_string();
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
                            expanded_names.push(Value::String(self.engine.expand_term(n, &prepared_context).await?.to_string()));
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
                            expanded_names.push(Value::String(self.engine.expand_term(n, &prepared_context).await?.to_string()));
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
                            let compacted = compact_type_iri(&self.engine, type_val, context).await;
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
                            compacted_names.push(Value::String(compact_attr_iri(&self.engine, n, context).await));
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
                            compacted_names.push(Value::String(compact_attr_iri(&self.engine, n, context).await));
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
