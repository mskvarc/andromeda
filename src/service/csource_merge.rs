//! Three-way merge of existing CSR state + partial update fragment into a
//! fully-resolved set of update values.
//!
//! Pure service-layer logic: no I/O, no SQL. The repository applies the
//! produced [`ResolvedCsrUpdate`] as an unconditional UPDATE.

use crate::{error::AndromedaError, service::csource_registration::store::CsrUpdateFragment};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation, ForwardingMode};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;

/// Fully-resolved CSR update values for a single row. Every field carries
/// the final value that will be written, with merge semantics already
/// applied.
#[derive(Debug)]
pub struct ResolvedCsrUpdate {
    /// New `registration_name` value (nullable).
    pub registration_name: Option<String>,
    /// New `description` value (nullable).
    pub description: Option<String>,
    /// New `information` array, serialised as JSON.
    pub information: Value,
    /// New `tenant` value.
    pub tenant: Option<TenantName>,
    /// New `observation_interval` object.
    pub observation_interval: Option<Value>,
    /// New `management_interval` object.
    pub management_interval: Option<Value>,
    /// New `expires_at` timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// New `endpoint` (required).
    pub endpoint: String,
    /// New `accept` MIME type.
    pub accept: Option<String>,
    /// New forwarding mode.
    pub mode: ForwardingMode,
    /// New supported operations list.
    pub operations: Option<Vec<CsrOperation>>,
    /// New scope strings.
    pub scope: Option<Vec<String>>,
    /// New refresh-rate duration string.
    pub refresh_rate: Option<Iso8601Duration>,
    /// New management object.
    pub management: Option<Value>,
    /// New context-source-info object.
    pub context_source_info: Option<Value>,
    /// New JSON-LD context identifier.
    pub jsonld_context: Option<String>,
    /// New Via alias pseudonym.
    pub context_source_alias: Option<String>,
    /// New non-standard-properties blob (after shallow merge).
    pub additional_properties: Option<Value>,
    /// New `location` geometry.
    pub location: Option<Geometry>,
    /// New `observation_space` geometry.
    pub observation_space: Option<Geometry>,
    /// New `operation_space` geometry.
    pub operation_space: Option<Geometry>,
}

/// Apply a partial `CsrUpdateFragment` on top of an existing CSR, producing
/// a fully-resolved set of column values for a repository UPDATE.
///
/// Every `.clone()` below is a fragment-side borrow → owned value for the
/// resolved output: the fragment is passed by `&`, the existing row is owned
/// and consumed, and the producer needs to yield a single owned
/// [`ResolvedCsrUpdate`] either way. There is no `&`/`Cow` alternative
/// because the repository takes `&ResolvedCsrUpdate` and the struct fields
/// are `Option<T>`/`String`/`Value` without lifetime parameters.
pub fn apply_fragment(existing: ContextSourceRegistration, fragment: &CsrUpdateFragment) -> Result<ResolvedCsrUpdate, AndromedaError> {
    let registration_name = fragment.registration_name.clone().unwrap_or(existing.registration_name); // clone: own fragment side; existing moves
    let description = fragment.description.clone().unwrap_or(existing.description); // clone: own fragment side; existing moves
    let information: Value = match fragment.information.as_ref() {
        Some(v) => v.clone(), // clone: own fragment Value; existing path serialises typed info
        None => serde_json::to_value(&existing.coverage.information)?,
    };
    let tenant = fragment.tenant.clone().unwrap_or(existing.tenant); // clone: own fragment side; existing moves
    let observation_interval = fragment.observation_interval.clone().unwrap_or(existing.coverage.observation_interval); // clone: own fragment side
    let management_interval = fragment.management_interval.clone().unwrap_or(existing.coverage.management_interval); // clone: own fragment side
    let expires_at = fragment.expires_at.unwrap_or(existing.lifecycle.expires_at);
    let endpoint = fragment.endpoint.clone().unwrap_or_else(|| existing.access.endpoint.to_string()); // clone: own fragment side; existing renders to the persisted form
    let accept = fragment
        .accept
        .clone() // clone: own fragment side; existing renders to the persisted form
        .unwrap_or_else(|| existing.access.accept.map(|media_type| media_type.to_string()));
    let mode = fragment.mode.unwrap_or_else(|| existing.access.mode.unwrap_or_default());
    let operations = fragment
        .operations
        .clone() // clone: own fragment side; existing path moves the typed operations
        .unwrap_or(existing.access.operations);
    let scope = fragment
        .scope
        .clone() // clone: own fragment side; existing path constructs fresh Vec<String>
        .unwrap_or_else(|| existing.coverage.scope.map(|v| v.into_iter().map(|s| s.to_string()).collect()));
    let refresh_rate = fragment.refresh_rate.unwrap_or(existing.access.refresh_rate);
    let management = fragment.management.clone().unwrap_or(existing.access.management); // clone: own fragment side
    let context_source_info = fragment.context_source_info.clone().unwrap_or(existing.access.context_source_info); // clone: own fragment side
    let jsonld_context = fragment
        .jsonld_context
        .clone() // clone: own fragment side; existing renders to the persisted form
        .unwrap_or_else(|| existing.access.jsonld_context.map(|url| url.to_string()));
    let context_source_alias = fragment.context_source_alias.clone().unwrap_or(existing.access.context_source_alias); // clone: own fragment side

    let additional_properties = match (existing.additional_properties, &fragment.additional_properties) {
        (Some(existing_val), Some(new_val)) => {
            let mut merged = existing_val;
            if let (Some(m), Some(n)) = (merged.as_object_mut(), new_val.as_object()) {
                for (k, v) in n {
                    m.insert(k.clone(), v.clone()); // clone: owned key/value for merged map insertion
                }
            }
            Some(merged)
        }
        (None, Some(new_val)) => Some(new_val.clone()), // clone: own fragment Value
        (Some(existing_val), None) => Some(existing_val),
        (None, None) => None,
    };

    let location = match fragment.location.as_ref() {
        Some(frag) => frag.clone(), // clone: own fragment side; fragment is borrowed
        None => existing
            .coverage
            .location
            .as_ref()
            .map(Geometry::from_geojson_value)
            .transpose()
            .map_err(|error| AndromedaError::csr_validation(format!("invalid existing location GeoJSON: {error}")))?,
    };
    let observation_space = match fragment.observation_space.as_ref() {
        Some(frag) => frag.clone(), // clone: own fragment side; fragment is borrowed
        None => existing
            .coverage
            .observation_space
            .as_ref()
            .map(Geometry::from_geojson_value)
            .transpose()
            .map_err(|error| AndromedaError::csr_validation(format!("invalid existing observationSpace GeoJSON: {error}")))?,
    };
    let operation_space = match fragment.operation_space.as_ref() {
        Some(frag) => frag.clone(), // clone: own fragment side; fragment is borrowed
        None => existing
            .coverage
            .operation_space
            .as_ref()
            .map(Geometry::from_geojson_value)
            .transpose()
            .map_err(|error| AndromedaError::csr_validation(format!("invalid existing operationSpace GeoJSON: {error}")))?,
    };

    Ok(ResolvedCsrUpdate {
        registration_name,
        description,
        information,
        tenant,
        observation_interval,
        management_interval,
        expires_at,
        endpoint,
        accept,
        mode,
        operations,
        scope,
        refresh_rate,
        management,
        context_source_info,
        jsonld_context,
        context_source_alias,
        additional_properties,
        location,
        observation_space,
        operation_space,
    })
}
