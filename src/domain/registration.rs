//! Context source registration types (ETSI GS CIM 009 clause 5.2.9).

use crate::{
    domain::{scope::ScopeBuf, serde_helpers, subscription::EntitySelector},
    tenancy::types::TenantName,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Forwarding mode for distributed operations (ETSI GS CIM 009 clause 4.3.6 / 5.2.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
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

/// CSR operation type (ETSI GS CIM 009 clause 4.20 / 5.2.9).
///
/// Known operations get typed variants; unknown/future operations are preserved
/// via `Other(String)` so the broker never silently drops unrecognised values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
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
    RetrieveTemporalEntity,
    QueryTemporalEntity,
    FederationOps,
    RedirectionOps,
    /// Any operation string not covered by a known variant.
    #[strum(default)]
    Other(String),
}

// Hand-written rather than `#[serde(rename_all = "camelCase")]`: a derived
// enum representation cannot funnel an unrecognised token into `Other(String)`,
// and dropping unknown operations would silently discard part of a
// registration. The wire casing comes from the `strum(serialize_all)` above, so
// there is still exactly one place that decides it.
impl Serialize for CsrOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CsrOperation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<CsrOperation, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // `#[strum(default)]` on `Other` makes the parse total, so the fallback
        // is unreachable rather than a swallowed failure.
        match raw.parse::<CsrOperation>() {
            Ok(operation) => Ok(operation),
            Err(_) => Ok(CsrOperation::Other(raw)),
        }
    }
}

/// Health status of a context source registration (ETSI GS CIM 009 clause 5.2.9, status member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrStatus {
    /// The context source is reachable and healthy.
    Ok,
    /// The context source is unreachable or unhealthy.
    Failed,
}

/// Information block describing what a context source provides (ETSI GS CIM 009 clause 5.2.10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationInfo {
    /// Entity type/id selectors this registration covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,
    /// Property names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_names: Option<Vec<String>>,
    /// Relationship names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship_names: Option<Vec<String>>,
}

fn default_csr_type() -> String {
    "ContextSourceRegistration".to_string()
}

/// NGSI-LD Context Source Registration (ETSI GS CIM 009 clause 5.2.9).
///
/// Describes an external context source and the entity types, attributes,
/// and operations it provides. Used by the broker for distributed/federated
/// query routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceRegistration {
    /// Registration identifier.
    pub id: String,
    /// JSON-LD type (always "ContextSourceRegistration").
    #[serde(rename = "type", default = "default_csr_type")]
    pub type_: String,
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_name: Option<String>,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Information blocks describing what this source provides.
    pub information: Vec<RegistrationInfo>,
    /// Tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<TenantName>,
    /// Observation interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_interval: Option<Value>,
    /// Management interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_interval: Option<Value>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_space: Option<Value>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_space: Option<Value>,
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
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
    pub scope: Option<Vec<ScopeBuf>>,
    /// Refresh rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<String>,
    /// Management metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<Value>,
    /// Context source info.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_info: Option<Value>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonld_context: Option<String>,
    /// Unique pseudonym for this context source, used for Via-based loop detection (clause 5.2.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_alias: Option<String>,
    /// Health status of the context source.
    pub status: CsrStatus,
    /// Number of successful notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_sent: Option<i32>,
    /// Number of failed notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_failed: Option<i32>,
    /// Timestamp of last successful notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of last failed notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub last_failure: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(with = "serde_helpers::rfc3339")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(with = "serde_helpers::rfc3339")]
    pub modified_at: DateTime<Utc>,
    /// Non-standard properties from the original CSR payload.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<Value>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    /// Labels of the PostgreSQL `csr_mode` enum, verbatim from
    /// `migrations/tenant/0003_types.sql`.
    const CSR_MODE_LABELS: [(&str, ForwardingMode); 4] = [
        ("inclusive", ForwardingMode::Inclusive),
        ("exclusive", ForwardingMode::Exclusive),
        ("redirect", ForwardingMode::Redirect),
        ("auxiliary", ForwardingMode::Auxiliary),
    ];

    /// Labels of the PostgreSQL `csr_status` enum.
    const CSR_STATUS_LABELS: [(&str, CsrStatus); 2] = [("ok", CsrStatus::Ok), ("failed", CsrStatus::Failed)];

    #[test]
    fn forwarding_mode_parses_from_its_postgres_label() {
        for (label, expected) in CSR_MODE_LABELS {
            pretty_assertions::assert_eq!(label.parse::<ForwardingMode>().expect(label), expected);
        }
    }

    #[test]
    fn forwarding_mode_renders_the_label_postgres_expects() {
        for (label, variant) in CSR_MODE_LABELS {
            pretty_assertions::assert_eq!(variant.to_string(), label);
            pretty_assertions::assert_eq!(<&str>::from(variant), label);
        }
    }

    #[test]
    fn forwarding_mode_round_trips_through_its_label_for_every_variant() {
        // Exhaustive over `EnumIter`, replacing the type-level guarantee
        // `sqlx::Type` used to give.
        for variant in ForwardingMode::iter() {
            pretty_assertions::assert_eq!(variant.to_string().parse::<ForwardingMode>().expect("round trip"), variant);
            assert!(
                CSR_MODE_LABELS.iter().any(|(_, known)| *known == variant),
                "{variant:?} has no PostgreSQL label in csr_mode"
            );
        }
    }

    #[test]
    fn an_unknown_forwarding_mode_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-mode".parse::<ForwardingMode>().is_err());
    }

    #[test]
    fn csr_status_parses_from_its_postgres_label() {
        for (label, expected) in CSR_STATUS_LABELS {
            pretty_assertions::assert_eq!(label.parse::<CsrStatus>().expect(label), expected);
        }
    }

    #[test]
    fn csr_status_renders_the_label_postgres_expects() {
        for (label, variant) in CSR_STATUS_LABELS {
            pretty_assertions::assert_eq!(variant.to_string(), label);
            pretty_assertions::assert_eq!(<&str>::from(variant), label);
        }
    }

    #[test]
    fn csr_status_round_trips_through_its_label_for_every_variant() {
        for variant in CsrStatus::iter() {
            pretty_assertions::assert_eq!(variant.to_string().parse::<CsrStatus>().expect("round trip"), variant);
            assert!(
                CSR_STATUS_LABELS.iter().any(|(_, known)| *known == variant),
                "{variant:?} has no PostgreSQL label in csr_status"
            );
        }
    }

    #[test]
    fn an_unknown_csr_status_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-status".parse::<CsrStatus>().is_err());
    }

    #[test]
    fn forwarding_mode_default_is_inclusive() {
        pretty_assertions::assert_eq!(ForwardingMode::default(), ForwardingMode::Inclusive);
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
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn csr_status_display_and_parse_round_trip() {
        for variant in [CsrStatus::Ok, CsrStatus::Failed] {
            let s = variant.to_string();
            let parsed = CsrStatus::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(parsed, variant, "round-trip failed for {variant}");
        }
    }

    #[test]
    fn csr_operation_known_round_trip() {
        let op = CsrOperation::RetrieveEntity;
        pretty_assertions::assert_eq!(op.to_string(), "retrieveEntity");
        pretty_assertions::assert_eq!("retrieveEntity".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_retrieve_temporal_entity_round_trip() {
        let op = CsrOperation::RetrieveTemporalEntity;
        pretty_assertions::assert_eq!(op.to_string(), "retrieveTemporalEntity");
        pretty_assertions::assert_eq!("retrieveTemporalEntity".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_query_temporal_entity_round_trip() {
        let op = CsrOperation::QueryTemporalEntity;
        pretty_assertions::assert_eq!(op.to_string(), "queryTemporalEntity");
        pretty_assertions::assert_eq!("queryTemporalEntity".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_other_preserved() {
        let op: CsrOperation = "customOp".parse().unwrap();
        pretty_assertions::assert_eq!(op, CsrOperation::Other("customOp".to_string()));
        pretty_assertions::assert_eq!(op.to_string(), "customOp");
    }
}
