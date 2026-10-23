//! The one document a `csf` filter reads of a registration (clauses 4.9 and
//! 5.2.9).
//!
//! Clause 4.9 makes the filter target "a Context Source Property member of the
//! `CSourceRegistration`, see Table 5.2.9-1", so the document holds every Table
//! 5.2.9-1 member a registration declares, with the value a retrieval carries
//! for it and keyed by the IRI the core `@context` expands its name to, then the
//! Context Source Properties the registrant added, under their expanded names.
//! `id` and `type` expand to JSON-LD keywords, which no filter names, and the
//! Table 5.2.9-2 health members and the system timestamps are not Context
//! Source Properties, so none of them is in it. Every reader of `csf` reads
//! this document: a forwarded read, a subscription to registrations, and the
//! stored copy registration discovery filters.

use crate::{
    key_value_pair::KeyValuePair,
    registration::{
        ContextSourceRegistration,
        context_source_properties::ContextSourceProperties,
        forwarding_mode::ForwardingMode,
        info::RegistrationInfo,
        operation::CsrOperation,
    },
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, piece::JsonPiece};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    duration::Iso8601Duration,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
    time::instant_serde,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iri_rs::Iri;
use serde::Serialize;
use typed_builder::TypedBuilder;

/// Each Table 5.2.9-1 member the document carries: the name a registration
/// spells it by, and the IRI the core `@context` expands that name to.
const TABLE_MEMBERS: [(&str, Iri<&str>); 19] = [
    (compact_properties::ENDPOINT, expanded_properties::ENDPOINT),
    (compact_properties::CONTEXT_SOURCE_INFO, expanded_properties::CONTEXT_SOURCE_INFO),
    (compact_properties::INFORMATION, expanded_properties::INFORMATION),
    (compact_properties::CONTEXT_SOURCE_ALIAS, expanded_properties::CONTEXT_SOURCE_ALIAS),
    (compact_properties::DESCRIPTION, expanded_properties::DESCRIPTION),
    (compact_properties::DATASET_ID, expanded_properties::DATASET_ID),
    (compact_properties::EXPIRES_AT, expanded_properties::EXPIRES_AT),
    (compact_properties::LOCATION, expanded_properties::LOCATION),
    (compact_properties::MANAGEMENT, expanded_properties::MANAGEMENT),
    (compact_properties::MANAGEMENT_INTERVAL, expanded_properties::MANAGEMENT_INTERVAL),
    (compact_properties::MODE, expanded_properties::MODE),
    (compact_properties::OBSERVATION_INTERVAL, expanded_properties::OBSERVATION_INTERVAL),
    (compact_properties::OBSERVATION_SPACE, expanded_properties::OBSERVATION_SPACE),
    (compact_properties::OPERATIONS, expanded_properties::OPERATIONS),
    (compact_properties::OPERATION_SPACE, expanded_properties::OPERATION_SPACE),
    (compact_properties::REFRESH_RATE, expanded_properties::REFRESH_RATE),
    (compact_properties::REGISTRATION_NAME, expanded_properties::REGISTRATION_NAME),
    (compact_properties::SCOPE, expanded_properties::SCOPE),
    (compact_properties::TENANT, expanded_properties::TENANT),
];

/// The members of one registration the document is built from, borrowed from a
/// stored registration or from the values a write is about to store.
///
/// Each member serializes under its Table 5.2.9-1 name exactly as the
/// registration does, so the document a write stores and the one a read builds
/// hold the same values.
#[derive(Debug, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourcePropertyMembers<'a> {
    /// `endpoint`.
    pub endpoint: &'a DereferenceableUri,
    /// `contextSourceInfo`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_info: Option<&'a [KeyValuePair]>,
    /// `information`.
    pub information: &'a [RegistrationInfo],
    /// `contextSourceAlias`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_alias: Option<&'a ContextSourcePseudonym>,
    /// `description`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a NonEmptyText>,
    /// `datasetId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<&'a [DatasetSelector]>,
    /// `expiresAt`.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "instant_serde::optional::serialize")]
    pub expires_at: Option<DateTime<Utc>>,
    /// `location`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<&'a Geometry>,
    /// `management`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management: Option<&'a RegistrationManagementInfo>,
    /// `managementInterval`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_interval: Option<&'a TimeInterval>,
    /// `mode`, which Table 5.2.9-1 assumes `inclusive` when a registration
    /// does not declare it, so it is always present.
    pub mode: ForwardingMode,
    /// `observationInterval`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_interval: Option<&'a TimeInterval>,
    /// `observationSpace`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_space: Option<&'a Geometry>,
    /// `operations`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<&'a [CsrOperation]>,
    /// `operationSpace`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_space: Option<&'a Geometry>,
    /// `refreshRate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<&'a Iso8601Duration>,
    /// `registrationName`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_name: Option<&'a NonEmptyText>,
    /// `scope`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<&'a [ScopePath]>,
    /// `tenant`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<&'a TenantName>,
    /// The Context Source Properties the registrant added, by expanded name.
    #[serde(skip)]
    pub added: &'a IndexMap<AttributeName, JsonLiteral>,
}

impl ContextSourcePropertyMembers<'_> {
    /// The document these members make.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError`] when a member does not write as JSON the
    /// literal admits.
    pub fn document(&self) -> Result<ContextSourcePropertyDocument, JsonLiteralError> {
        let written = admitted(self)?;
        let written = written.piece();
        let document: IndexMap<&str, JsonPiece<'_>> = TABLE_MEMBERS
            .iter()
            .filter_map(|(name, iri)| written.member(name).map(|value| (iri.as_str(), value)))
            .chain(self.added.iter().map(|(name, value)| (name.as_str(), value.piece())))
            .collect();
        Ok(ContextSourcePropertyDocument(admitted(&document)?))
    }
}

/// `value` written as JSON and admitted as a literal.
fn admitted(value: &impl Serialize) -> Result<JsonLiteral, JsonLiteralError> {
    JsonLiteral::admit(&serde_json::to_string(value).map_err(|source| JsonLiteralError::Syntax { source })?)
}

impl ContextSourceRegistration {
    /// The members of this registration the `csf` document is built from.
    #[must_use]
    pub fn property_members(&self) -> ContextSourcePropertyMembers<'_> {
        ContextSourcePropertyMembers::builder()
            .endpoint(&self.access.endpoint)
            .context_source_info(self.access.context_source_info.as_deref())
            .information(&self.coverage.information)
            .context_source_alias(self.access.context_source_alias.as_ref())
            .description(self.description.as_ref())
            .dataset_id(self.coverage.dataset_id.as_deref())
            .expires_at(self.lifecycle.expires_at)
            .location(self.coverage.location.as_ref())
            .management(self.access.management.as_ref())
            .management_interval(self.coverage.management_interval.as_ref())
            .mode(self.access.mode.unwrap_or_default())
            .observation_interval(self.coverage.observation_interval.as_ref())
            .observation_space(self.coverage.observation_space.as_ref())
            .operations(self.access.operations.as_deref())
            .operation_space(self.coverage.operation_space.as_ref())
            .refresh_rate(self.access.refresh_rate.as_ref())
            .registration_name(self.registration_name.as_ref())
            .scope(self.coverage.scope.as_deref())
            .tenant(self.tenant.as_ref())
            .added(&self.additional_properties)
            .build()
    }
}

/// The Context Source Properties of one registration as a JSON object keyed by
/// expanded name.
#[derive(Debug)]
pub struct ContextSourcePropertyDocument(JsonLiteral);

impl ContextSourcePropertyDocument {
    /// The properties as the `q` evaluator reads them.
    #[must_use]
    pub fn properties(&self) -> ContextSourceProperties<'_> {
        ContextSourceProperties::of_object(self.0.piece())
    }

    /// The document as the JSON it is stored as.
    #[must_use]
    pub fn into_literal(self) -> JsonLiteral {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::{ContextSourceRegistration, context_source_property_document::TABLE_MEMBERS};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_query::{
        filter::{eval::evaluate, parser::parse},
        queried_entity::linked_entities::NoLinkedEntities,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    /// One registration declaring every Table 5.2.9-1 member, every Table
    /// 5.2.9-2 member, both system timestamps and one added property.
    fn fully_populated() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "registrationName": "vehicles",
            "description": "vehicle telemetry source",
            "tenant": "acme",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}], "propertyNames": ["https://example.org/speed"]}],
            "datasetId": ["urn:ngsi-ld:dataset:1", "@none"],
            "scope": ["/Madrid"],
            "location": {"type": "Point", "coordinates": [13.0, 52.0]},
            "observationSpace": {"type": "Point", "coordinates": [13.0, 52.0]},
            "operationSpace": {"type": "Point", "coordinates": [13.0, 52.0]},
            "observationInterval": {"startAt": "2024-01-01T00:00:00Z"},
            "managementInterval": {"startAt": "2024-01-01T00:00:00Z", "endAt": "2025-01-01T00:00:00Z"},
            "endpoint": "https://source.example.com/",
            "mode": "exclusive",
            "operations": ["retrieveEntity", "updateEntity"],
            "refreshRate": "PT1H",
            "management": {"cacheDuration": "PT10S", "cooldown": 500, "localOnly": false, "timeout": 3000},
            "contextSourceInfo": [{"key": "Authorization", "value": "Bearer x"}],
            "contextSourceAlias": "source-1",
            "status": "failed",
            "timesSent": 3,
            "timesFailed": 1,
            "lastSuccess": "2024-02-01T00:00:00Z",
            "lastFailure": "2024-01-15T00:00:00Z",
            "expiresAt": "2030-12-31T23:59:59Z",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z",
            "https://example.org/tier": {"type": "Property", "value": "gold"}
        }))
        .expect("a registration")
    }

    fn document_of(registration: &ContextSourceRegistration) -> Value {
        let document = registration.property_members().document().expect("the document builds");
        serde_json::from_str(document.into_literal().text()).expect("the document is JSON")
    }

    /// A JSON value as the data model compares it: numbers by value.
    fn literal(value: &Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("a JSON literal")
    }

    fn holds(q: &str, registration: &ContextSourceRegistration) -> bool {
        let document = registration.property_members().document().expect("the document builds");
        evaluate(&parse(q).expect("the filter parses"), document.properties(), NoLinkedEntities)
    }

    /// Clause 4.9: every Table 5.2.9-1 member a registration declares is a
    /// Context Source Property, keyed by the IRI its core name expands to and
    /// spelled as the registration spells it, followed by the added ones.
    #[test]
    fn the_property_document_carries_table_5_2_9_1_members_under_core_iris_and_the_added_properties() {
        let registration = fully_populated();
        let document = document_of(&registration);
        let written = serde_json::to_value(&registration).expect("the registration writes");
        let keys: BTreeSet<&str> = document.as_object().expect("an object").keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> = TABLE_MEMBERS.iter().map(|(_, iri)| iri.as_str()).chain(["https://example.org/tier"]).collect();
        pretty_assertions::assert_eq!(keys, expected);
        for (name, iri) in TABLE_MEMBERS {
            pretty_assertions::assert_eq!(literal(&document[iri.as_str()]), literal(&written[name]), "{name}");
        }
        pretty_assertions::assert_eq!(document["https://uri.etsi.org/ngsi-ld/mode"], json!("exclusive"));
        pretty_assertions::assert_eq!(document["http://purl.org/dc/terms/description"], json!("vehicle telemetry source"));
        pretty_assertions::assert_eq!(document["https://example.org/tier"], json!({"type": "Property", "value": "gold"}));
    }

    /// Table 5.2.9-2 members report what the broker observed, and `id`,
    /// `type`, `createdAt` and `modifiedAt` are not Context Source Properties.
    #[test]
    fn the_property_document_leaves_out_health_and_system_members() {
        let document = document_of(&fully_populated());
        let members = document.as_object().expect("an object");
        for left_out in [
            "id",
            "type",
            "@id",
            "@type",
            "status",
            "timesSent",
            "timesFailed",
            "lastSuccess",
            "lastFailure",
            "createdAt",
            "modifiedAt",
            "https://uri.etsi.org/ngsi-ld/status",
            "https://uri.etsi.org/ngsi-ld/timesSent",
            "https://uri.etsi.org/ngsi-ld/timesFailed",
            "https://uri.etsi.org/ngsi-ld/lastSuccess",
            "https://uri.etsi.org/ngsi-ld/lastFailure",
            "https://uri.etsi.org/ngsi-ld/createdAt",
            "https://uri.etsi.org/ngsi-ld/modifiedAt",
        ] {
            assert!(!members.contains_key(left_out), "{left_out}");
        }
    }

    /// Table 5.2.9-1 assumes `inclusive` for a registration declaring no
    /// `mode`, and a filter reads the assumed mode.
    #[test]
    fn a_registration_declaring_no_mode_reads_as_inclusive() {
        let registration: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:2",
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["https://example.org/speed"]}],
            "endpoint": "https://source.example.com/",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("a registration");
        assert!(holds("\"https://uri.etsi.org/ngsi-ld/mode\"==\"inclusive\"", &registration));
        assert!(!holds("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"", &registration));
    }

    /// Clause 4.9: a filter reads a Table 5.2.9-1 member and an added property
    /// alike.
    #[test]
    fn a_filter_reads_the_mode_and_an_added_property() {
        let registration = fully_populated();
        assert!(holds("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"", &registration));
        assert!(holds("\"https://example.org/tier\"==\"gold\"", &registration));
        assert!(!holds("\"https://example.org/tier\"==\"silver\"", &registration));
        assert!(!holds("\"https://uri.etsi.org/ngsi-ld/status\"", &registration));
    }
}
