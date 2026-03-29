//! Context source registration types (ETSI GS CIM 009 clause 5.2.9).

use crate::domain::{serde_helpers, subscription::EntitySelector};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumString};

/// Forwarding mode for distributed operations (clause 4.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ForwardingMode {
    /// The registration supplements local data (default).
    #[default]
    Inclusive,
    /// The registration is the exclusive source for matched entities.
    Exclusive,
    /// Requests are redirected to the context source.
    Redirect,
    /// The registration provides auxiliary/fallback data.
    Auxiliary,
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
    /// Returns the camelCase string for this operation.
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

impl std::fmt::Display for CsrOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

/// Health status of a context source registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrStatus {
    /// The context source is reachable and healthy.
    Ok,
    /// The context source is unreachable or unhealthy.
    Failed,
}

/// Information block describing what a context source provides (clause 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationInfo {
    /// Entity type/id selectors this registration covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,
    /// Property names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none", rename = "propertyNames")]
    pub property_names: Option<Vec<String>>,
    /// Relationship names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none", rename = "relationshipNames")]
    pub relationship_names: Option<Vec<String>>,
}

/// A full context source registration (clause 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSourceRegistration {
    /// Registration identifier.
    pub id: String,
    /// JSON-LD type (always "ContextSourceRegistration").
    #[serde(rename = "type")]
    pub type_: String,
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none", rename = "registrationName")]
    pub registration_name: Option<String>,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Information blocks describing what this source provides.
    pub information: Vec<RegistrationInfo>,
    /// Tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Observation interval.
    #[serde(skip_serializing_if = "Option::is_none", rename = "observationInterval")]
    pub observation_interval: Option<Value>,
    /// Management interval.
    #[serde(skip_serializing_if = "Option::is_none", rename = "managementInterval")]
    pub management_interval: Option<Value>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none", rename = "observationSpace")]
    pub observation_space: Option<Value>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none", rename = "operationSpace")]
    pub operation_space: Option<Value>,
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// The context source endpoint URL.
    pub endpoint: String,
    /// Preferred accept media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    /// Forwarding mode for distributed operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ForwardingMode>,
    /// Operations this registration supports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<CsrOperation>>,
    /// Scope values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
    /// Refresh rate.
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshRate")]
    pub refresh_rate: Option<String>,
    /// Management metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<Value>,
    /// Context source info.
    #[serde(skip_serializing_if = "Option::is_none", rename = "contextSourceInfo")]
    pub context_source_info: Option<Value>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,
    /// Health status of the context source.
    pub status: CsrStatus,
    /// Number of successful notifications.
    #[serde(skip_serializing_if = "Option::is_none", rename = "timesSent")]
    pub times_sent: Option<i32>,
    /// Number of failed notifications.
    #[serde(skip_serializing_if = "Option::is_none", rename = "timesFailed")]
    pub times_failed: Option<i32>,
    /// Timestamp of last successful notification.
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSuccess", with = "serde_helpers::rfc3339_option", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of last failed notification.
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastFailure", with = "serde_helpers::rfc3339_option", default)]
    pub last_failure: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(rename = "createdAt", with = "serde_helpers::rfc3339")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(rename = "modifiedAt", with = "serde_helpers::rfc3339")]
    pub modified_at: DateTime<Utc>,
    /// Non-standard properties from the original CSR payload.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<Value>,
}

/// Partial context source registration for PATCH update operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrFragment {
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none", rename = "registrationName")]
    pub registration_name: Option<String>,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Information blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Observation interval.
    #[serde(skip_serializing_if = "Option::is_none", rename = "observationInterval")]
    pub observation_interval: Option<Value>,
    /// Management interval.
    #[serde(skip_serializing_if = "Option::is_none", rename = "managementInterval")]
    pub management_interval: Option<Value>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none", rename = "observationSpace")]
    pub observation_space: Option<Value>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none", rename = "operationSpace")]
    pub operation_space: Option<Value>,
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// The context source endpoint URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Preferred accept media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    /// Forwarding mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ForwardingMode>,
    /// Operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<CsrOperation>>,
    /// Scope values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
    /// Refresh rate.
    #[serde(skip_serializing_if = "Option::is_none", rename = "refreshRate")]
    pub refresh_rate: Option<String>,
    /// Management metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<Value>,
    /// Context source info.
    #[serde(skip_serializing_if = "Option::is_none", rename = "contextSourceInfo")]
    pub context_source_info: Option<Value>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn forwarding_mode_default_is_inclusive() {
        assert_eq!(ForwardingMode::default(), ForwardingMode::Inclusive);
    }

    #[test]
    fn forwarding_mode_display_and_parse_round_trip() {
        for variant in [
            ForwardingMode::Inclusive,
            ForwardingMode::Exclusive,
            ForwardingMode::Redirect,
            ForwardingMode::Auxiliary,
        ] {
            let s = variant.to_string();
            let parsed = ForwardingMode::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn csr_status_display_and_parse_round_trip() {
        for variant in [CsrStatus::Ok, CsrStatus::Failed] {
            let s = variant.to_string();
            let parsed = CsrStatus::from_str(&s).unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn csr_operation_known_round_trip() {
        let op = CsrOperation::RetrieveEntity;
        assert_eq!(op.to_string(), "retrieveEntity");
        assert_eq!("retrieveEntity".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_other_preserved() {
        let op: CsrOperation = "customOp".parse().unwrap();
        assert_eq!(op, CsrOperation::Other("customOp".to_string()));
        assert_eq!(op.to_string(), "customOp");
    }
}
