//! Repository input parameters for context source registrations.

use crate::{domain::registration::ForwardingMode, tenancy::types::TenantName};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Parameters for creating a new context source registration.
#[derive(Debug)]
pub struct CreateCsrParams<'a> {
    /// Human-readable name of the registration.
    pub registration_name: Option<&'a str>,
    /// Free-text description of the registration.
    pub description: Option<&'a str>,
    /// Registration information entries describing provided entities/attributes.
    pub information: Option<&'a Value>,
    /// Tenant identifier for multi-tenancy.
    pub tenant: Option<&'a str>,
    /// Expected interval between observations.
    pub observation_interval: Option<&'a Value>,
    /// Expected interval between management interactions.
    pub management_interval: Option<&'a Value>,
    /// Pre-extracted GeoJSON string for `location`.
    pub location_geojson: Option<String>,
    /// Pre-extracted GeoJSON string for `observationSpace`.
    pub observation_space_geojson: Option<String>,
    /// Pre-extracted GeoJSON string for `operationSpace`.
    pub operation_space_geojson: Option<String>,
    /// Expiration timestamp for the registration.
    pub expires_at: Option<DateTime<Utc>>,
    /// Base endpoint URL of the context source.
    pub endpoint: Option<&'a str>,
    /// Preferred MIME type for responses from the context source.
    pub accept: Option<&'a str>,
    /// Forwarding mode for distributed operations.
    pub mode: ForwardingMode,
    /// NGSI-LD operations supported by the context source.
    pub operations: Option<Vec<String>>,
    /// Scope values for the registration.
    pub scope: Option<Vec<String>>,
    /// ISO 8601 duration between context source refreshes.
    pub refresh_rate: Option<&'a str>,
    /// Management information for the registration.
    pub management: Option<&'a Value>,
    /// Additional context source metadata entries.
    pub context_source_info: Option<&'a Value>,
    /// JSON-LD context associated with this registration.
    pub jsonld_context: Option<&'a str>,
    /// Unique pseudonym for Via-based loop detection.
    pub context_source_alias: Option<&'a str>,
    /// Non-standard properties preserved from the original request.
    pub additional_properties: Option<Value>,
}

impl CreateCsrParams<'_> {
    /// Keys recognized as standard NGSI-LD CSR properties.
    pub const STANDARD_KEYS: &'static [&'static str] = &[
        "id",
        "type",
        "@context",
        "registrationName",
        "description",
        "information",
        "tenant",
        "observationInterval",
        "managementInterval",
        "location",
        "observationSpace",
        "operationSpace",
        "expiresAt",
        "endpoint",
        "accept",
        "mode",
        "operations",
        "scope",
        "refreshRate",
        "management",
        "contextSourceInfo",
        "jsonldContext",
        "contextSourceAlias",
        "status",
        "timesSent",
        "timesFailed",
        "lastSuccess",
        "lastFailure",
        "createdAt",
        "modifiedAt",
    ];
}

/// Typed fragment for CSR updates. `None` = field not provided (keep existing).
/// `Some(None)` = explicitly null (clear field). `Some(Some(v))` = new value.
#[derive(Debug)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    pub registration_name: Option<Option<String>>,
    /// Updated description.
    pub description: Option<Option<String>>,
    /// Updated registration information entries.
    pub information: Option<Value>,
    /// Updated tenant identifier.
    pub tenant: Option<Option<TenantName>>,
    /// Updated observation interval.
    pub observation_interval: Option<Option<Value>>,
    /// Updated management interval.
    pub management_interval: Option<Option<Value>>,
    /// Updated GeoJSON location (as serialized string).
    pub location: Option<Option<String>>,
    /// Updated GeoJSON observation space (as serialized string).
    pub observation_space: Option<Option<String>>,
    /// Updated GeoJSON operation space (as serialized string).
    pub operation_space: Option<Option<String>>,
    /// Updated expiration timestamp.
    pub expires_at: Option<Option<DateTime<Utc>>>,
    /// Updated endpoint URL.
    pub endpoint: Option<String>,
    /// Updated accepted MIME type.
    pub accept: Option<Option<String>>,
    /// Updated forwarding mode.
    pub mode: Option<ForwardingMode>,
    /// Updated supported operations.
    pub operations: Option<Option<Vec<String>>>,
    /// Updated scope values.
    pub scope: Option<Option<Vec<String>>>,
    /// Updated refresh rate duration.
    pub refresh_rate: Option<Option<String>>,
    /// Updated management information.
    pub management: Option<Option<Value>>,
    /// Updated context source metadata entries.
    pub context_source_info: Option<Option<Value>>,
    /// Updated JSON-LD context reference.
    pub jsonld_context: Option<Option<String>>,
    /// Updated context source alias pseudonym.
    pub context_source_alias: Option<Option<String>>,
    /// Updated non-standard properties.
    pub additional_properties: Option<Value>,
}
