//! The body of Create and Update Context Source Registration (ETSI GS CIM 009
//! clauses 5.9.2 and 5.9.3).
//!
//! Both operations read the same members of Table 5.2.9-1, each as a
//! [`NullableMember`]: present, `null` (which clause 5.5.8 lets an update use to
//! remove the member) or absent. The body decodes straight from the request text
//! into the types that own each member's restriction -- `NonEmptyText`,
//! `TimeInterval`, `RegistrationManagementInfo`, `DatasetSelector` -- so a value
//! outside Table 5.2.9-1 fails the decoding, naming the member, as
//! `BadRequestData`. What the body carries beyond Table 5.2.9-1 is the
//! registration's Context Source Properties, held as data (clause 4.9).

use crate::{
    csr_member::CsrMember,
    error::RegistrationError,
    geometry::RegistrationGeometry,
    mode_rules::default_operations,
    nullable_member::NullableMember,
    registration_validation_reason::RegistrationValidationReason,
    store::{create_params::CreateCsrParams, update_fragment::CsrUpdateFragment},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistrationType, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    request_body::RequestBody,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;

/// The members a Create or Update Context Source Registration carries.
#[derive(Debug, Default)]
pub struct RegistrationBody {
    /// The identifier the client names the registration by; a creation without
    /// one is given an identifier by the broker (clause 5.9.2.4).
    pub(crate) id: Option<Identifier>,
    /// `type`: cardinality 1 on creation, and what an update may restate.
    pub(crate) type_: NullableMember<ContextSourceRegistrationType>,
    /// `registrationName`.
    pub(crate) registration_name: NullableMember<NonEmptyText>,
    /// `description`.
    pub(crate) description: NullableMember<NonEmptyText>,
    /// `information`: cardinality 1 on creation.
    pub(crate) information: NullableMember<Vec<RegistrationInfo>>,
    /// `datasetId`.
    pub(crate) dataset_id: NullableMember<Vec<DatasetSelector>>,
    /// `tenant`.
    pub(crate) tenant: NullableMember<TenantName>,
    /// `observationInterval`.
    pub(crate) observation_interval: NullableMember<TimeInterval>,
    /// `managementInterval`.
    pub(crate) management_interval: NullableMember<TimeInterval>,
    /// `location`.
    pub(crate) location: NullableMember<RegistrationGeometry>,
    /// `observationSpace`.
    pub(crate) observation_space: NullableMember<RegistrationGeometry>,
    /// `operationSpace`.
    pub(crate) operation_space: NullableMember<RegistrationGeometry>,
    /// `expiresAt`.
    pub(crate) expires_at: NullableMember<DateTime<Utc>>,
    /// `endpoint`: cardinality 1 on creation.
    pub(crate) endpoint: NullableMember<DereferenceableUri>,
    /// `accept`.
    pub(crate) accept: NullableMember<MediaTypeBuf>,
    /// `mode`.
    pub(crate) mode: NullableMember<ForwardingMode>,
    /// `operations`.
    pub(crate) operations: NullableMember<Vec<CsrOperation>>,
    /// `scope`.
    pub(crate) scope: NullableMember<Vec<ScopePath>>,
    /// `refreshRate`.
    pub(crate) refresh_rate: NullableMember<Iso8601Duration>,
    /// `management`.
    pub(crate) management: NullableMember<RegistrationManagementInfo>,
    /// `contextSourceInfo`.
    pub(crate) context_source_info: NullableMember<Vec<KeyValuePair>>,
    /// `contextSourceAlias`.
    pub(crate) context_source_alias: NullableMember<ContextSourcePseudonym>,
    /// The Context Source Properties (clause 4.9): every member beyond the ones
    /// Table 5.2.9-1 defines, in the order the body wrote them.
    pub(crate) additional_properties: IndexMap<AttributeName, JsonLiteral>,
}

// Decoded straight from the request text by the body's own reading, which
// skips the top-level `@context`.
impl RequestBody for RegistrationBody {}

/// Table 5.2.9-1: `information` is an array of at least one block; an empty
/// array is not allowed.
fn non_empty_information(information: Vec<RegistrationInfo>) -> Result<Vec<RegistrationInfo>, RegistrationError> {
    if information.is_empty() {
        return Err(RegistrationError::Validation(RegistrationValidationReason::InformationMustNotBeEmpty));
    }
    Ok(information)
}

/// The geometry a member holds, as the registration stores it.
fn stored_geometry(member: NullableMember<RegistrationGeometry>) -> NullableMember<Geometry> {
    match member {
        NullableMember::Absent => NullableMember::Absent,
        NullableMember::Cleared => NullableMember::Cleared,
        NullableMember::Set(written) => NullableMember::Set(written.into_geometry()),
    }
}

impl RegistrationBody {
    /// The geometries the body writes, by the member that holds each.
    pub(crate) fn geometries(&self) -> impl Iterator<Item = (CsrMember, &Geometry)> {
        [
            (CsrMember::Location, &self.location),
            (CsrMember::ObservationSpace, &self.observation_space),
            (CsrMember::OperationSpace, &self.operation_space),
        ]
        .into_iter()
        .filter_map(|(member, written)| match written {
            NullableMember::Set(written) => Some((member, written.geometry())),
            NullableMember::Absent | NullableMember::Cleared => None,
        })
    }

    /// The input one Create Context Source Registration writes (clause 5.9.2),
    /// the members the body carries read as the registration stores them.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberRequired`] for a missing
    /// `type`, `endpoint` or `information` (Table 5.2.9-1 gives them cardinality
    /// 1), [`RegistrationValidationReason::MemberMustNotBeNull`] for a member
    /// carried as `null`, and [`RegistrationValidationReason::InformationMustNotBeEmpty`]
    /// for an empty `information`.
    pub(crate) fn into_create_params(self) -> Result<CreateCsrParams, RegistrationError> {
        let RegistrationBody {
            id: _,
            type_,
            registration_name,
            description,
            information,
            dataset_id,
            tenant,
            observation_interval,
            management_interval,
            location,
            observation_space,
            operation_space,
            expires_at,
            endpoint,
            accept,
            mode,
            operations,
            scope,
            refresh_rate,
            management,
            context_source_info,
            context_source_alias,
            additional_properties,
        } = self;
        type_.required(CsrMember::Type)?;
        let information = non_empty_information(information.required(CsrMember::Information)?)?;
        // Table 5.2.9-1: absent `mode` is `inclusive`. The mode is read before
        // `operations` because it decides which default set applies to it.
        let mode = mode.present(CsrMember::Mode)?.unwrap_or_default();
        // The effective value is what gets stored, so retrieval and routing read
        // the same thing the admission check saw.
        let operations = operations.present(CsrMember::Operations)?.unwrap_or_else(|| default_operations(mode).to_vec());
        Ok(CreateCsrParams {
            registration_name: registration_name.present(CsrMember::RegistrationName)?,
            description: description.present(CsrMember::Description)?,
            information,
            dataset_id: dataset_id.present(CsrMember::DatasetId)?,
            tenant: tenant.present(CsrMember::Tenant)?,
            observation_interval: observation_interval.present(CsrMember::ObservationInterval)?,
            management_interval: management_interval.present(CsrMember::ManagementInterval)?,
            location: stored_geometry(location).present(CsrMember::Location)?,
            observation_space: stored_geometry(observation_space).present(CsrMember::ObservationSpace)?,
            operation_space: stored_geometry(operation_space).present(CsrMember::OperationSpace)?,
            expires_at: expires_at.present(CsrMember::ExpiresAt)?,
            endpoint: endpoint.required(CsrMember::Endpoint)?,
            accept: accept.present(CsrMember::Accept)?,
            mode,
            operations,
            scope: scope.present(CsrMember::Scope)?,
            refresh_rate: refresh_rate.present(CsrMember::RefreshRate)?,
            management: management.present(CsrMember::Management)?,
            context_source_info: context_source_info.present(CsrMember::ContextSourceInfo)?,
            context_source_alias: context_source_alias.present(CsrMember::ContextSourceAlias)?,
            additional_properties,
        })
    }

    /// The fragment one Update Context Source Registration applies (clauses
    /// 5.9.3 and 5.5.8).
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberMustNotBeNull`] for a
    /// `type`, `information`, `endpoint` or `mode` carried as `null`, which
    /// clause 5.5.8 does not let an update remove, and
    /// [`RegistrationValidationReason::InformationMustNotBeEmpty`] for an empty
    /// `information`.
    pub(crate) fn into_update_fragment(self) -> Result<CsrUpdateFragment, RegistrationError> {
        let RegistrationBody {
            id: _,
            type_,
            registration_name,
            description,
            information,
            dataset_id,
            tenant,
            observation_interval,
            management_interval,
            location,
            observation_space,
            operation_space,
            expires_at,
            endpoint,
            accept,
            mode,
            operations,
            scope,
            refresh_rate,
            management,
            context_source_info,
            context_source_alias,
            additional_properties,
        } = self;
        // A fragment may omit `type`, but one it carries has to be the
        // registration type, which its reading has already checked (clause 5.2.9).
        type_.present(CsrMember::Type)?;
        Ok(CsrUpdateFragment {
            registration_name,
            description,
            information: information.present(CsrMember::Information)?.map(non_empty_information).transpose()?,
            dataset_id,
            tenant,
            observation_interval,
            management_interval,
            location: stored_geometry(location),
            observation_space: stored_geometry(observation_space),
            operation_space: stored_geometry(operation_space),
            expires_at,
            endpoint: endpoint.present(CsrMember::Endpoint)?,
            accept,
            mode: mode.present(CsrMember::Mode)?,
            operations,
            scope,
            refresh_rate,
            management,
            context_source_info,
            context_source_alias,
            additional_properties,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{nullable_member::NullableMember, registration_body::RegistrationBody, store::create_params::CreateCsrParams};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{registration::operation::CsrOperation, request_body::RequestBody};
    use serde_json::{Value, json};

    fn baseline() -> Value {
        json!({
            "type": "ContextSourceRegistration",
            "endpoint": "https://source.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}]
        })
    }

    fn with(member: &str, value: Value) -> Value {
        let mut payload = baseline();
        payload[member] = value;
        payload
    }

    /// The body `document` is as a request carrying it as text.
    fn body_of(document: &Value) -> Result<RegistrationBody, serde_json::Error> {
        RegistrationBody::from_request_text(document.to_string().as_bytes())
    }

    fn create_of(document: &Value) -> Result<CreateCsrParams, String> {
        body_of(document)
            .map_err(|error| error.to_string())?
            .into_create_params()
            .map_err(|error| error.to_string())
    }

    #[test]
    fn create_materialises_the_clause_4_20_default_operation_group() {
        let params = create_of(&baseline()).expect("valid registration");
        pretty_assertions::assert_eq!(params.operations, vec![CsrOperation::FederationOps]);
    }

    #[test]
    fn create_preserves_an_explicit_operation_subset() {
        let params = create_of(&with("operations", json!(["retrieveEntity", "queryEntity"]))).expect("valid registration");
        pretty_assertions::assert_eq!(params.operations, vec![CsrOperation::RetrieveEntity, CsrOperation::QueryEntity]);
    }

    #[test]
    fn create_requires_type_endpoint_and_information() {
        for member in ["type", "endpoint", "information"] {
            let mut payload = baseline();
            payload.as_object_mut().expect("object").remove(member);
            assert!(create_of(&payload).is_err(), "missing {member} must be rejected");
        }
    }

    #[test]
    fn create_rejects_every_member_outside_table_5_2_9_1() {
        for (member, value) in [
            ("type", json!("Foo")),
            ("endpoint", json!("###not a uri###")),
            ("information", json!([])),
            ("information", json!([{}])),
            ("information", json!([{"entities": []}])),
            ("information", json!([{"propertyNames": []}])),
            ("information", json!([{"relationshipNames": []}])),
            ("information", json!([{"entities": [{"id": "urn:ngsi-ld:Vehicle:V1"}]}])),
            ("information", json!([{"entities": [{"type": "Vehicle", "id": "not a uri"}]}])),
            ("information", json!([{"entities": [{"type": "Vehicle", "idPattern": "[unterminated"}]}])),
            ("contextSourceAlias", json!("")),
            ("description", json!("")),
            ("registrationName", json!("")),
            ("datasetId", json!(["not a uri"])),
            ("expiresAt", json!("not-a-date")),
            ("location", json!({"type": "Point", "coordinates": "nope"})),
            ("observationSpace", json!({"type": "Polygon", "coordinates": 5})),
            ("operationSpace", json!({"type": "Point"})),
            ("mode", json!("foo")),
            ("operations", json!(["notAnOperation"])),
            ("refreshRate", json!("5 minutes")),
            ("managementInterval", json!({"endAt": "2030-01-01T00:00:00Z"})),
            (
                "observationInterval",
                json!({"startAt": "2030-01-01T00:00:00Z", "endAt": "2020-01-01T00:00:00Z"}),
            ),
            ("management", json!({"cacheDuration": "10s"})),
            ("management", json!({"cooldown": 0})),
            ("management", json!({"cooldown": -5})),
            ("management", json!({"timeout": 0})),
            ("management", json!({"timeout": -1})),
            ("registrationName", Value::Null),
            ("scope", Value::Null),
            ("location", Value::Null),
        ] {
            assert!(create_of(&with(member, value.clone())).is_err(), "{member} = {value} must be rejected"); // clone: reported on failure
        }
    }

    #[test]
    fn create_accepts_every_valid_optional_member() {
        let mut payload = baseline();
        payload["datasetId"] = json!(["urn:ngsi-ld:ds:1", "@none"]);
        payload["management"] = json!({"localOnly": true, "timeout": 3000, "cooldown": 500, "cacheDuration": "PT10S"});
        payload["managementInterval"] = json!({"startAt": "2020-01-01T00:00:00Z"});
        payload["refreshRate"] = json!("PT5M");
        payload["scope"] = json!(["/Madrid/Gardens"]);
        payload["contextSourceAlias"] = json!("broker-alias-x");
        payload["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}});
        payload["customThing"] = json!({"kept": true});
        payload["status"] = json!("ok");
        let params = create_of(&payload).expect("valid registration");
        pretty_assertions::assert_eq!(params.dataset_id.as_ref().map(Vec::len), Some(2));
        assert!(params.management.is_some());
        assert!(params.location.is_some());
        assert!(params.management_interval.is_some_and(|interval| interval.end_at.is_none()));
        let extras: Vec<(&str, &str)> = params.additional_properties.iter().map(|(name, value)| (name.as_str(), value.text())).collect();
        pretty_assertions::assert_eq!(
            extras,
            vec![("customThing", r#"{"kept":true}"#)],
            "a read-only member is no Context Source Property"
        );
    }

    /// Clause 5.2.9: a member the specification does not define is data, and
    /// keeps the order the body wrote it in.
    #[test]
    fn the_unknown_members_of_a_body_are_carried_in_the_order_written() {
        let body = RegistrationBody::from_request_text(
            br#"{"type": "ContextSourceRegistration", "zeta": 1, "endpoint": "https://source.example.org", "alpha": {"b": [1.50]}, "information": [{"propertyNames": ["speed"]}]}"#,
        )
        .expect("a body");
        let extras: Vec<(&str, &str)> = body.additional_properties.iter().map(|(name, value)| (name.as_str(), value.text())).collect();
        // The numbers of a literal are spelled the canonical way.
        pretty_assertions::assert_eq!(extras, vec![("zeta", "1"), ("alpha", r#"{"b":[1.5]}"#)]);
    }

    /// A top-level `@context` names the context and is no member of the body.
    #[test]
    fn a_top_level_context_is_no_context_source_property() {
        let body = RegistrationBody::from_request_text(
            br#"{"@context": "https://example.org/ctx", "type": "ContextSourceRegistration", "endpoint": "https://source.example.org", "information": [{"propertyNames": ["speed"]}]}"#,
        )
        .expect("a body");
        assert!(body.additional_properties.is_empty());
    }

    /// Clause 5.2.9: the registration identifier is a URI no longer than an
    /// identifier may be, and the refusal names the member.
    #[test]
    fn a_registration_id_past_the_identifier_limit_is_refused() {
        let prefix = "urn:ngsi-ld:ContextSourceRegistration:";
        let id = format!("{prefix}{}", "a".repeat(1025 - prefix.len()));
        let error = body_of(&with("id", json!(id))).expect_err("an over-long id");
        assert!(error.to_string().contains("1025 bytes"), "{error}");
    }

    #[test]
    fn a_registration_id_within_the_limit_is_read_verbatim() {
        let body = body_of(&with("id", json!("urn:ngsi-ld:ContextSourceRegistration:1"))).expect("a body");
        pretty_assertions::assert_eq!(body.id.as_ref().map(Identifier::as_str), Some("urn:ngsi-ld:ContextSourceRegistration:1"));
    }

    #[test]
    fn update_distinguishes_absent_null_and_present_members() {
        let fragment = body_of(&json!({"description": null, "management": {"timeout": 10}}))
            .expect("a body")
            .into_update_fragment()
            .expect("valid fragment");
        assert!(matches!(fragment.description, NullableMember::Cleared));
        assert!(matches!(fragment.registration_name, NullableMember::Absent));
        assert!(matches!(fragment.management, NullableMember::Set(_)));
    }

    #[test]
    fn update_rejects_a_member_outside_its_restriction() {
        for member in [
            json!({"management": {"timeout": 0}}),
            json!({"description": ""}),
            json!({"information": [{"entities": []}]}),
            json!({"information": []}),
            json!({"type": "Subscription"}),
        ] {
            let refused = body_of(&member)
                .map_err(|error| error.to_string())
                .and_then(|body| body.into_update_fragment().map_err(|error| error.to_string()));
            assert!(refused.is_err(), "{member} must be rejected");
        }
    }

    /// Clause 5.5.8 lets an update remove optional members only: the members of
    /// cardinality 1 and the forwarding mode stay.
    #[test]
    fn update_refuses_to_remove_type_information_endpoint_or_mode() {
        for member in ["type", "information", "endpoint", "mode"] {
            let mut document = json!({});
            document[member] = Value::Null;
            let refused = body_of(&document).expect("a body").into_update_fragment();
            assert!(refused.is_err(), "{member} = null must be rejected");
        }
    }
}
