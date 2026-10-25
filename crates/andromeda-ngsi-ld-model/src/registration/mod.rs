//! The Context Source Registration itself (ETSI GS CIM 009 clause 5.2.9).
//!
//! Clause 5.2.9 defines one resource out of five groups of members, and each
//! group answers a different question: what the source covers, how the broker
//! reaches it, what has been observed about it, when the registration applies,
//! and what identifies it. They live in a module apiece, and this one holds
//! only the resource that flattens them together. What a `csf` filter reads of
//! a registration is [`context_source_property_document`], and the filters a
//! request places on registrations beside clause 5.12 matching are
//! [`selection`].

pub mod access;
pub mod context_source_properties;
pub mod context_source_property_document;
pub mod coverage;
pub mod forwarding_mode;
pub mod health;
pub mod info;
pub mod lifecycle;
pub mod operation;
pub mod selection;
pub mod status;

use crate::registration::{access::ContextSourceAccess, coverage::RegistrationCoverage, health::RegistrationHealth, lifecycle::RegistrationLifecycle};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, constant_string, identifier::Identifier, non_empty_text::NonEmptyText, tenant::TenantName};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(ContextSourceRegistrationType, "ContextSourceRegistration");

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
    pub id: Identifier,
    /// JSON-LD type (always `ContextSourceRegistration`; clause 5.2.9 cardinality 1,
    /// so a document without it is not a registration).
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: ContextSourceRegistrationType,
    /// Human-readable name.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub registration_name: Option<NonEmptyText>,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub description: Option<NonEmptyText>,
    /// Tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
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
    /// The Context Source Properties of the registration (clause 4.9): the
    /// members beyond Table 5.2.9-1, which the specification does not define and
    /// the broker carries as data, in the order they were written.
    ///
    /// Declared last: it is the catch-all flatten, and it collects whatever the
    /// groups above did not claim.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default = Default::default())]
    pub additional_properties: IndexMap<AttributeName, JsonLiteral>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation, status::CsrStatus};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
            "datasetId": ["urn:ngsi-ld:dataset:1", "@none"],
            "scope": ["/Madrid"],
            "location": {"type": "Point", "coordinates": [13, 52]},
            "observationSpace": {"type": "Point", "coordinates": [13, 52]},
            "operationSpace": {"type": "Point", "coordinates": [13, 52]},
            "observationInterval": {"startAt": "2024-01-01T00:00:00Z"},
            "managementInterval": {"startAt": "2024-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00Z"},
            "endpoint": "https://source.example.com/",
            "accept": "application/ld+json",
            "mode": "inclusive",
            "operations": ["retrieveEntity", "updateEntity"],
            "refreshRate": "PT1H",
            "management": {"cacheDuration": "PT10S", "cooldown": 500, "localOnly": false, "timeout": 3000},
            "contextSourceInfo": [{"key": "Authorization", "value": "Bearer x"}],
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
        let extras = registration.additional_properties;
        let custom = "customThing".parse::<AttributeName>().expect("a valid name");
        pretty_assertions::assert_eq!(extras.get(&custom).map(JsonLiteral::text), Some(r#"{"kept":true}"#));
        pretty_assertions::assert_eq!(extras.len(), 1, "a grouped member must not leak into the catch-all");
    }

    /// Clause 5.2.9: a registration written as a client posted it reads into
    /// the typed model and writes back as the same document, a member the
    /// specification does not define included.
    #[test]
    fn a_context_source_registration_round_trips_through_its_typed_model() {
        let posted = fully_populated_registration().to_string();
        let registration: ContextSourceRegistration = serde_json::from_str(&posted).expect("registration parses");
        let written = serde_json::to_string(&registration).expect("registration writes");
        let reread: serde_json::Value = serde_json::from_str(&written).expect("the written body is JSON");
        pretty_assertions::assert_eq!(reread, fully_populated_registration());
        assert!(
            written.contains(r#""customThing":{"kept":true}"#),
            "the unknown member is written as data: {written}"
        );
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
    fn a_registration_without_a_type_is_rejected_because_clause_5_2_9_makes_it_mandatory() {
        let mut document = fully_populated_registration();
        document.as_object_mut().expect("object").remove("type");
        assert!(serde_json::from_value::<ContextSourceRegistration>(document).is_err());
    }

    #[test]
    fn an_empty_registration_name_or_description_is_rejected() {
        for member in ["registrationName", "description"] {
            let mut document = fully_populated_registration();
            document[member] = serde_json::json!("");
            assert!(
                serde_json::from_value::<ContextSourceRegistration>(document).is_err(),
                "empty {member} must be rejected"
            );
        }
    }

    #[test]
    fn a_registration_info_with_no_member_or_an_empty_member_is_rejected() {
        assert!(serde_json::from_value::<RegistrationInfo>(serde_json::json!({})).is_err());
        assert!(serde_json::from_value::<RegistrationInfo>(serde_json::json!({"entities": []})).is_err());
        assert!(serde_json::from_value::<RegistrationInfo>(serde_json::json!({"propertyNames": []})).is_err());
        assert!(serde_json::from_value::<RegistrationInfo>(serde_json::json!({"relationshipNames": []})).is_err());
        assert!(serde_json::from_value::<RegistrationInfo>(serde_json::json!({"propertyNames": ["speed"]})).is_ok());
    }

    #[test]
    fn a_registration_info_yields_its_names_from_both_axes() {
        let info: RegistrationInfo =
            serde_json::from_value(serde_json::json!({"propertyNames": ["speed"], "relationshipNames": ["isParked"]})).expect("info parses");
        let names: Vec<&str> = info.attribute_names().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(names, vec!["speed", "isParked"]);
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
