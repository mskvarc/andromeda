//! NGSI-LD Context Source Registration types (ETSI GS CIM 009 clause 5.2.9).

use crate::{subscription::EntitySelector, time_format};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Forwarding mode for Context Source Registrations (ETSI GS CIM 009 clause 4.3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardingMode {
    Exclusive,
    Redirect,
    Inclusive,
    Auxiliary,
}

impl Default for ForwardingMode {
    fn default() -> Self {
        Self::Inclusive
    }
}

impl fmt::Display for ForwardingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exclusive => f.write_str("exclusive"),
            Self::Redirect => f.write_str("redirect"),
            Self::Inclusive => f.write_str("inclusive"),
            Self::Auxiliary => f.write_str("auxiliary"),
        }
    }
}

impl std::str::FromStr for ForwardingMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "exclusive" => Ok(Self::Exclusive),
            "redirect" => Ok(Self::Redirect),
            "inclusive" => Ok(Self::Inclusive),
            "auxiliary" => Ok(Self::Auxiliary),
            _ => Err(format!("invalid forwarding mode: {}", s)),
        }
    }
}

/// CSR operation type (ETSI GS CIM 009 clause 5.2.9).
///
/// Known operations get typed variants; unknown/future operations are preserved
/// via `Other(String)` so the broker never silently drops unrecognised values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CsrOperation {
    RetrieveEntity,
    QueryEntity,
    QueryBatch,
    CreateEntity,
    DeleteEntity,
    MergeEntity,
    ReplaceEntity,
    AppendAttrs,
    UpdateAttrs,
    PartialAttrUpdate,
    DeleteAttrs,
    CreateBatch,
    UpsertBatch,
    UpdateBatch,
    DeleteBatch,
    MergeBatch,
    FederationOps,
    RedirectionOps,
    /// Any operation string not covered by a known variant.
    Other(String),
}

impl CsrOperation {
    pub fn as_str(&self) -> &str {
        match self {
            Self::RetrieveEntity => "retrieveEntity",
            Self::QueryEntity => "queryEntity",
            Self::QueryBatch => "queryBatch",
            Self::CreateEntity => "createEntity",
            Self::DeleteEntity => "deleteEntity",
            Self::MergeEntity => "mergeEntity",
            Self::ReplaceEntity => "replaceEntity",
            Self::AppendAttrs => "appendAttrs",
            Self::UpdateAttrs => "updateAttrs",
            Self::PartialAttrUpdate => "partialAttrUpdate",
            Self::DeleteAttrs => "deleteAttrs",
            Self::CreateBatch => "createBatch",
            Self::UpsertBatch => "upsertBatch",
            Self::UpdateBatch => "updateBatch",
            Self::DeleteBatch => "deleteBatch",
            Self::MergeBatch => "mergeBatch",
            Self::FederationOps => "federationOps",
            Self::RedirectionOps => "redirectionOps",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for CsrOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for CsrOperation {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "retrieveEntity" => Self::RetrieveEntity,
            "queryEntity" => Self::QueryEntity,
            "queryBatch" => Self::QueryBatch,
            "createEntity" => Self::CreateEntity,
            "deleteEntity" => Self::DeleteEntity,
            "mergeEntity" => Self::MergeEntity,
            "replaceEntity" => Self::ReplaceEntity,
            "appendAttrs" => Self::AppendAttrs,
            "updateAttrs" => Self::UpdateAttrs,
            "partialAttrUpdate" => Self::PartialAttrUpdate,
            "deleteAttrs" => Self::DeleteAttrs,
            "createBatch" => Self::CreateBatch,
            "upsertBatch" => Self::UpsertBatch,
            "updateBatch" => Self::UpdateBatch,
            "deleteBatch" => Self::DeleteBatch,
            "mergeBatch" => Self::MergeBatch,
            "federationOps" => Self::FederationOps,
            "redirectionOps" => Self::RedirectionOps,
            other => Self::Other(other.to_string()),
        })
    }
}

impl Serialize for CsrOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CsrOperation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().unwrap()) // Infallible
    }
}

/// Health status of a Context Source Registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CsrStatus {
    Ok,
    Failed,
}

impl fmt::Display for CsrStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("ok"),
            Self::Failed => f.write_str("failed"),
        }
    }
}

impl std::str::FromStr for CsrStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(Self::Ok),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("invalid CSR status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSourceRegistration {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(skip_serializing_if = "Option::is_none", rename = "registrationName")]
    pub registration_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub information: Vec<RegistrationInfo>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "observationInterval")]
    pub observation_interval: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "managementInterval")]
    pub management_interval: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "observationSpace")]
    pub observation_space: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "operationSpace")]
    pub operation_space: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "time_format::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    pub endpoint: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ForwardingMode>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<CsrOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshRate")]
    pub refresh_rate: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "contextSourceInfo")]
    pub context_source_info: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,

    pub status: CsrStatus,

    #[serde(skip_serializing_if = "Option::is_none", rename = "timesSent")]
    pub times_sent: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "timesFailed")]
    pub times_failed: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSuccess", with = "time_format::rfc3339_option", default)]
    pub last_success: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "lastFailure", with = "time_format::rfc3339_option", default)]
    pub last_failure: Option<DateTime<Utc>>,

    #[serde(rename = "createdAt", with = "time_format::rfc3339")]
    pub created_at: DateTime<Utc>,

    #[serde(rename = "modifiedAt", with = "time_format::rfc3339")]
    pub modified_at: DateTime<Utc>,

    /// Non-standard properties from the original CSR payload (e.g. custom attributes).
    /// Flattened into the top-level object during serialization.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "propertyNames")]
    pub property_names: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "relationshipNames")]
    pub relationship_names: Option<Vec<String>>,
}

/// Fragment for updating a CSR (all fields optional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrFragment {
    #[serde(skip_serializing_if = "Option::is_none", rename = "registrationName")]
    pub registration_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub information: Option<Vec<RegistrationInfo>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "observationInterval")]
    pub observation_interval: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "managementInterval")]
    pub management_interval: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "observationSpace")]
    pub observation_space: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "operationSpace")]
    pub operation_space: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "time_format::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ForwardingMode>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<CsrOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshRate")]
    pub refresh_rate: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "contextSourceInfo")]
    pub context_source_info: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,
}
