//! The registration store port, and the parameter shapes it takes.
//!
//! `update` takes a fully resolved value set rather than a fragment: clause
//! 5.10.4 defines a registration update as a merge against the stored
//! registration, and that merge is policy. The store applies a decided row.

use crate::{core::error::AndromedaError, service::csource_merge::ResolvedCsrUpdate};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, ForwardingMode};
use andromeda_ngsi_ld_query::csr::compiled::CompiledCsrQuery;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
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
    /// The `location` geometry.
    pub location: Option<Geometry>,
    /// The `observationSpace` geometry.
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` geometry.
    pub operation_space: Option<Geometry>,
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
    pub refresh_rate: Option<Iso8601Duration>,
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
    /// Updated `location` geometry (`Some(None)` clears, `Some(Some(_))` replaces).
    pub location: Option<Option<Geometry>>,
    /// Updated `observationSpace` geometry.
    pub observation_space: Option<Option<Geometry>>,
    /// Updated `operationSpace` geometry.
    pub operation_space: Option<Option<Geometry>>,
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
    pub refresh_rate: Option<Option<Iso8601Duration>>,
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

/// Storing context source registrations (clause 5.10).
#[async_trait]
pub trait RegistrationStore: Send + Sync + 'static {
    /// Whether a registration with this identifier exists.
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError>;

    /// Persist a new registration.
    async fn create(&self, id: &str, params: &CreateCsrParams<'_>) -> Result<(), AndromedaError>;

    /// Retrieve a registration by identifier.
    async fn get(&self, id: &str) -> Result<Option<ContextSourceRegistration>, AndromedaError>;

    /// List registrations with pagination, returning the entries and total count.
    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError>;

    /// Run a compiled registration query, returning matches and the total count.
    async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError>;

    /// Replace a registration with a fully resolved value set. Returns rows affected.
    async fn update(&self, id: &str, values: &ResolvedCsrUpdate) -> Result<u64, AndromedaError>;

    /// Registrations that may serve an entity type and/or identifier, excluding
    /// expired ones (clause 5.10.2).
    async fn find_matching(&self, entity_type: Option<&str>, entity_id: Option<&str>) -> Result<Vec<ContextSourceRegistration>, AndromedaError>;

    /// One page of non-expired registrations, keyset-paginated on the identifier.
    ///
    /// `after` is the last identifier of the previous page, exclusive.
    async fn find_all_active(&self, after: Option<&str>, limit: u32) -> Result<Vec<ContextSourceRegistration>, AndromedaError>;

    /// Delete a registration by identifier. Returns rows affected.
    async fn delete(&self, id: &str) -> Result<u64, AndromedaError>;
}
