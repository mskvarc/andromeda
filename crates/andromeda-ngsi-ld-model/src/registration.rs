//! Context source registration types (ETSI GS CIM 009 clause 5.2.9).

use crate::{key_value_pair::KeyValuePair, subscription::EntitySelector};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    constant_string,
    context_source_pseudonym::ContextSourcePseudonym,
    rfc3339,
    scope::grammar::ScopeBuf,
    tenant::TenantName,
    uri::Uri,
};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;
use url::Url;

constant_string!(ContextSourceRegistrationType, "ContextSourceRegistration");

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrOperation {
    /// Create an entity (clause 5.6.1).
    CreateEntity,
    /// Update an entity's attribute collection (clause 5.6.2).
    UpdateEntity,
    /// Append entity attributes (clause 5.6.3).
    AppendAttrs,
    /// Partially update one attribute (clause 5.6.4).
    UpdateAttrs,
    /// Delete an entity attribute (clause 5.6.5).
    DeleteAttrs,
    /// Delete an entity (clause 5.6.6).
    DeleteEntity,
    /// Batch entity creation (clause 5.6.7).
    CreateBatch,
    /// Batch entity upsert (clause 5.6.8).
    UpsertBatch,
    /// Batch entity update (clause 5.6.9).
    UpdateBatch,
    /// Batch entity deletion (clause 5.6.10).
    DeleteBatch,
    /// Create or update temporal evolution (clause 5.6.11).
    UpsertTemporal,
    /// Add attributes to temporal evolution (clause 5.6.12).
    AppendAttrsTemporal,
    /// Delete attributes from temporal evolution (clause 5.6.13).
    DeleteAttrsTemporal,
    /// Partially update a temporal attribute instance (clause 5.6.14).
    UpdateAttrInstanceTemporal,
    /// Delete a temporal attribute instance (clause 5.6.15).
    DeleteAttrInstanceTemporal,
    /// Delete temporal evolution (clause 5.6.16).
    DeleteTemporal,
    /// Merge-patch an entity (clause 5.6.17).
    MergeEntity,
    /// Replace an entity (clause 5.6.18).
    ReplaceEntity,
    /// Replace an attribute (clause 5.6.19).
    ReplaceAttrs,
    /// Batch entity merge (clause 5.6.20).
    MergeBatch,
    /// Purge entities (clause 5.6.21).
    PurgeEntity,
    /// Retrieve one entity (clause 5.7.1).
    RetrieveEntity,
    /// Query entities, excluding the batch endpoint (clause 5.7.2).
    QueryEntity,
    /// Query entities through the batch endpoint (clause 5.7.2).
    QueryBatch,
    /// Retrieve one temporal entity (clause 5.7.3).
    RetrieveTemporal,
    /// Query temporal entities (clause 5.7.4).
    QueryTemporal,
    /// Retrieve available entity types (clause 5.7.5).
    RetrieveEntityTypes,
    /// Retrieve details of available entity types (clause 5.7.6).
    RetrieveEntityTypeDetails,
    /// Retrieve available entity type information (clause 5.7.7).
    RetrieveEntityTypeInfo,
    /// Retrieve available attributes (clause 5.7.8).
    RetrieveAttrTypes,
    /// Retrieve details of available attributes (clause 5.7.9).
    RetrieveAttrTypeDetails,
    /// Retrieve available attribute information (clause 5.7.10).
    RetrieveAttrTypeInfo,
    /// Create a subscription (clause 5.8.1).
    CreateSubscription,
    /// Update a subscription (clause 5.8.2).
    UpdateSubscription,
    /// Retrieve a subscription (clause 5.8.3).
    RetrieveSubscription,
    /// Query subscriptions (clause 5.8.4).
    QuerySubscription,
    /// Delete a subscription (clause 5.8.5).
    DeleteSubscription,
    /// Retrieve an entity map (clause 5.14.1).
    RetrieveEntityMap,
    /// Update an entity map (clause 5.14.2).
    UpdateEntityMap,
    /// Delete an entity map (clause 5.14.3).
    DeleteEntityMap,
    /// Create an entity map for an entity query (clause 5.14.4).
    CreateEntityMapQueryEntity,
    /// Create an entity map for a temporal query (clause 5.14.5).
    CreateEntityMapQueryTemporal,
    /// Retrieve context-source identity information (clause 5.15.1).
    RetrieveContextSourceIdentity,
    /// The federation operation group from Table 4.20-2.
    FederationOps,
    /// The association operation group from Table 4.20-2.
    AssociationOps,
    /// The update operation group from Table 4.20-2.
    UpdateOps,
    /// The retrieve operation group from Table 4.20-2.
    RetrieveOps,
    /// The redirection operation group from Table 4.20-2.
    RedirectionOps,
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
    pub property_names: Option<Vec<AttributeName>>,
    /// Relationship names provided by this context source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship_names: Option<Vec<AttributeName>>,
}

/// What a context source covers: the entity and attribute selectors it answers
/// for, and the space and time its data spans (ETSI GS CIM 009 clauses 5.2.9
/// and 5.2.10).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationCoverage {
    /// Information blocks describing what this source provides.
    pub information: Vec<RegistrationInfo>,
    /// Scope values.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub scope: Option<Vec<ScopeBuf>>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub location: Option<Value>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub observation_space: Option<Value>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub operation_space: Option<Value>,
    /// Interval the source's observations span (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub observation_interval: Option<Value>,
    /// Interval the source's management data spans (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub management_interval: Option<Value>,
}

/// How the broker reaches a context source and which operations it may send
/// there (ETSI GS CIM 009 clauses 4.20 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceAccess {
    /// The context source endpoint URL.
    pub endpoint: Url,
    /// Preferred accept media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub accept: Option<MediaTypeBuf>,
    /// Forwarding mode for distributed operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub mode: Option<ForwardingMode>,
    /// Operations this registration supports.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub operations: Option<Vec<CsrOperation>>,
    /// Refresh rate, as an ISO 8601 duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub refresh_rate: Option<Iso8601Duration>,
    /// Management metadata (clause 5.2.43).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub management: Option<Value>,
    /// Context source info.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// JSON-LD context for term expansion.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub jsonld_context: Option<Url>,
    /// Unique pseudonym for this context source, used for Via-based loop detection (clause 5.2.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub context_source_alias: Option<ContextSourcePseudonym>,
}

/// What the broker has observed about a context source's availability
/// (ETSI GS CIM 009 clause 5.2.9, `status` and the delivery counters).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationHealth {
    /// Health status of the context source.
    pub status: CsrStatus,
    /// Number of successful notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub times_sent: Option<i32>,
    /// Number of failed notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub times_failed: Option<i32>,
    /// Timestamp of last successful notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default)]
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of last failed notification.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default)]
    pub last_failure: Option<DateTime<Utc>>,
}

/// When a registration was written and when it stops applying
/// (ETSI GS CIM 009 clauses 4.8 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationLifecycle {
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(with = "rfc3339::datetime")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(with = "rfc3339::datetime")]
    pub modified_at: DateTime<Utc>,
}

/// NGSI-LD Context Source Registration (ETSI GS CIM 009 clause 5.2.9).
///
/// Describes an external context source and the entity types, attributes,
/// and operations it provides. Used by the broker for distributed/federated
/// query routing. The members are grouped by what changes them:
/// [`RegistrationCoverage`] follows what the source answers for,
/// [`ContextSourceAccess`] how the broker talks to it, [`RegistrationHealth`]
/// what the broker has observed, and [`RegistrationLifecycle`] when the
/// registration applies. Every group is flattened, so the wire form stays the
/// single flat object clause 5.2.9 defines.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceRegistration {
    /// Registration identifier (clause 5.2.9: a URI, not necessarily an NGSI-LD URN).
    pub id: Uri,
    /// JSON-LD type (always `ContextSourceRegistration`).
    #[serde(rename = "type", default)]
    #[builder(default)]
    pub type_: ContextSourceRegistrationType,
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub registration_name: Option<String>,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub description: Option<String>,
    /// Tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub tenant: Option<TenantName>,
    /// What the context source answers for.
    #[serde(flatten)]
    pub coverage: RegistrationCoverage,
    /// How the broker reaches the context source.
    #[serde(flatten)]
    pub access: ContextSourceAccess,
    /// What the broker has observed about the context source.
    #[serde(flatten)]
    pub health: RegistrationHealth,
    /// When the registration applies.
    #[serde(flatten)]
    pub lifecycle: RegistrationLifecycle,
    /// Non-standard properties from the original CSR payload.
    ///
    /// Declared last: it is the catch-all flatten, and it collects whatever the
    /// groups above did not claim.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub additional_properties: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn an_unknown_forwarding_mode_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-mode".parse::<ForwardingMode>().is_err());
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

    /// One registration carrying every member clause 5.2.9 defines plus a
    /// non-standard one, so the flattened groups and the catch-all are proven
    /// to leave the wire form flat.
    fn fully_populated_registration() -> serde_json::Value {
        serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "registrationName": "vehicles",
            "description": "vehicle telemetry source",
            "tenant": "acme",
            "information": [{"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]}],
            "scope": ["/Madrid"],
            "location": {"type": "Point", "coordinates": [13.0, 52.0]},
            "observationSpace": {"type": "Point", "coordinates": [13.0, 52.0]},
            "operationSpace": {"type": "Point", "coordinates": [13.0, 52.0]},
            "observationInterval": {"start": "2024-01-01T00:00:00Z"},
            "managementInterval": {"start": "2024-01-01T00:00:00Z"},
            "endpoint": "https://source.example.com/",
            "accept": "application/ld+json",
            "mode": "inclusive",
            "operations": ["retrieveEntity", "updateEntity"],
            "refreshRate": "PT1H",
            "management": {"localOnly": false},
            "contextSourceInfo": [{"key": "Authorization", "value": "Bearer x"}],
            "jsonldContext": "https://example.com/context.jsonld",
            "contextSourceAlias": "source-1",
            "status": "ok",
            "timesSent": 3,
            "timesFailed": 1,
            "lastSuccess": "2024-02-01T00:00:00Z",
            "lastFailure": "2024-01-15T00:00:00Z",
            "expiresAt": "2030-12-31T23:59:59Z",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z",
            "customThing": {"kept": true}
        })
    }

    #[test]
    fn a_registration_reserialises_to_the_flat_object_it_was_read_from() {
        let original = fully_populated_registration();
        // clone: the original is compared against the round-tripped value below
        let registration: ContextSourceRegistration = serde_json::from_value(original.clone()).expect("registration parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&registration).expect("serialises"), original);
    }

    #[test]
    fn a_registration_keeps_a_non_standard_member_in_its_catch_all() {
        let registration: ContextSourceRegistration = serde_json::from_value(fully_populated_registration()).expect("registration parses");
        let extras = registration.additional_properties.expect("catch-all present");
        pretty_assertions::assert_eq!(extras["customThing"], serde_json::json!({"kept": true}));
        assert!(extras.get("endpoint").is_none(), "a grouped member must not leak into the catch-all");
        assert!(extras.get("status").is_none(), "a grouped member must not leak into the catch-all");
    }

    #[test]
    fn a_registration_builder_produces_the_same_value_as_the_parsed_form() {
        let parsed: ContextSourceRegistration = serde_json::from_value(fully_populated_registration()).expect("registration parses");
        let built = ContextSourceRegistration::builder()
            .id(parsed.id.clone()) // clone: the parsed value is compared against the built one below
            .registration_name(parsed.registration_name.clone()) // clone: same reason
            .description(parsed.description.clone()) // clone: same reason
            .tenant(parsed.tenant.clone()) // clone: same reason
            .coverage(parsed.coverage.clone()) // clone: same reason
            .access(parsed.access.clone()) // clone: same reason
            .health(parsed.health.clone()) // clone: same reason
            .lifecycle(parsed.lifecycle.clone()) // clone: same reason
            .additional_properties(parsed.additional_properties.clone()) // clone: same reason
            .build();
        pretty_assertions::assert_eq!(
            serde_json::to_value(&built).expect("serialises"),
            serde_json::to_value(&parsed).expect("serialises")
        );
    }

    #[test]
    fn csr_operation_known_round_trip() {
        let op = CsrOperation::RetrieveEntity;
        pretty_assertions::assert_eq!(op.to_string(), "retrieveEntity");
        pretty_assertions::assert_eq!("retrieveEntity".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_retrieve_temporal_entity_round_trip() {
        let op = CsrOperation::RetrieveTemporal;
        pretty_assertions::assert_eq!(op.to_string(), "retrieveTemporal");
        pretty_assertions::assert_eq!("retrieveTemporal".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_query_temporal_entity_round_trip() {
        let op = CsrOperation::QueryTemporal;
        pretty_assertions::assert_eq!(op.to_string(), "queryTemporal");
        pretty_assertions::assert_eq!("queryTemporal".parse::<CsrOperation>().unwrap(), op);
    }

    #[test]
    fn csr_operation_unknown_token_is_rejected() {
        assert!("customOp".parse::<CsrOperation>().is_err());
        assert!(serde_json::from_str::<CsrOperation>(r#""customOp""#).is_err());
    }
}
