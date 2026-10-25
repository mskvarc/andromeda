//! The body of Create and Update Context Source Registration (ETSI GS CIM 009
//! clauses 5.9.2 and 5.9.3).
//!
//! Both operations read the same members of Table 5.2.9-1, each as a
//! [`Clearable`]: a value, the NGSI-LD Null (which clause 5.5.8 lets an update
//! use to delete the member) or absent. A JSON `null` is no value of any member
//! (clause 5.2.1) and is refused as the body is read. The body decodes straight from the request text
//! into the types that own each member's restriction -- `NonEmptyText`,
//! `TimeInterval`, `RegistrationManagementInfo`, `DatasetSelector` -- so a value
//! outside Table 5.2.9-1 fails the decoding, naming the member, as
//! `BadRequestData`. What the body carries beyond Table 5.2.9-1 is the
//! registration's Context Source Properties, held as data (clause 4.9); one
//! written as the NGSI-LD Null deletes the stored property of its name.

use crate::{
    csr_member::CsrMember,
    error::RegistrationError,
    geometry::RegistrationGeometry,
    member_presence::{present, required},
    mode_rules::default_operations,
    registration_validation_reason::RegistrationValidationReason,
    store::{create_params::CreateCsrParams, update_fragment::CsrUpdateFragment},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    duration::Iso8601Duration,
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
    time::date_time::NgsiLdDateTime,
};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistrationType, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    request_body::RequestBody,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use indexmap::IndexMap;
use mediatype::MediaTypeBuf;

/// The members a Create or Update Context Source Registration carries.
#[derive(Debug, Default)]
pub struct RegistrationBody {
    /// The identifier the client names the registration by; a creation without
    /// one is given an identifier by the broker (clause 5.9.2.4).
    pub(crate) id: Option<Identifier>,
    /// `type`: cardinality 1 on creation, and what an update may restate.
    pub(crate) type_: Clearable<ContextSourceRegistrationType>,
    /// `registrationName`.
    pub(crate) registration_name: Clearable<NonEmptyText>,
    /// `description`.
    pub(crate) description: Clearable<NonEmptyText>,
    /// `information`: cardinality 1 on creation.
    pub(crate) information: Clearable<Vec<RegistrationInfo>>,
    /// `datasetId`.
    pub(crate) dataset_id: Clearable<Vec<DatasetSelector>>,
    /// `tenant`.
    pub(crate) tenant: Clearable<TenantName>,
    /// `observationInterval`.
    pub(crate) observation_interval: Clearable<TimeInterval>,
    /// `managementInterval`.
    pub(crate) management_interval: Clearable<TimeInterval>,
    /// `location`.
    pub(crate) location: Clearable<RegistrationGeometry>,
    /// `observationSpace`.
    pub(crate) observation_space: Clearable<RegistrationGeometry>,
    /// `operationSpace`.
    pub(crate) operation_space: Clearable<RegistrationGeometry>,
    /// `expiresAt`, read as every instant a request carries (clause 4.6.3).
    pub(crate) expires_at: Clearable<NgsiLdDateTime>,
    /// `endpoint`: cardinality 1 on creation.
    pub(crate) endpoint: Clearable<DereferenceableUri>,
    /// `accept`.
    pub(crate) accept: Clearable<MediaTypeBuf>,
    /// `mode`.
    pub(crate) mode: Clearable<ForwardingMode>,
    /// `operations`.
    pub(crate) operations: Clearable<Vec<CsrOperation>>,
    /// `scope`.
    pub(crate) scope: Clearable<Vec<ScopePath>>,
    /// `refreshRate`.
    pub(crate) refresh_rate: Clearable<Iso8601Duration>,
    /// `management`.
    pub(crate) management: Clearable<RegistrationManagementInfo>,
    /// `contextSourceInfo`.
    pub(crate) context_source_info: Clearable<Vec<KeyValuePair>>,
    /// `contextSourceAlias`.
    pub(crate) context_source_alias: Clearable<ContextSourcePseudonym>,
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
fn stored_geometry(member: Clearable<RegistrationGeometry>) -> Clearable<Geometry> {
    member.map(RegistrationGeometry::into_geometry)
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
            Clearable::Set(written) => Some((member, written.geometry())),
            Clearable::Absent | Clearable::NgsiLdNull => None,
        })
    }

    /// Refuse a creation body that wrote a Context Source Property as the
    /// NGSI-LD Null, which has nothing to delete (clause 5.2.1).
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::PropertyMustNotBeNull`] naming
    /// the first such property.
    pub(crate) fn refuse_ngsi_ld_null_properties(&self) -> Result<(), RegistrationError> {
        match self.additional_properties.iter().find(|(_, value)| value.is_null_sentinel()) {
            Some((name, _)) => Err(RegistrationError::Validation(RegistrationValidationReason::PropertyMustNotBeNull {
                name: name.clone(), // clone: the refusal owns the name the body keeps
            })),
            None => Ok(()),
        }
    }

    /// The input one Create Context Source Registration writes (clause 5.9.2),
    /// the members the body carries read as the registration stores them.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberRequired`] for a missing
    /// `type`, `endpoint` or `information` (Table 5.2.9-1 gives them cardinality
    /// 1), [`RegistrationValidationReason::MemberMustNotBeNull`] for a member
    /// written as the NGSI-LD Null, and [`RegistrationValidationReason::InformationMustNotBeEmpty`]
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
        required(type_, CsrMember::Type)?;
        let information = non_empty_information(required(information, CsrMember::Information)?)?;
        // Table 5.2.9-1: absent `mode` is `inclusive`. The mode is read before
        // `operations` because it decides which default set applies to it.
        let mode = present(mode, CsrMember::Mode)?.unwrap_or_default();
        // The effective value is what gets stored, so retrieval and routing read
        // the same thing the admission check saw.
        let operations = present(operations, CsrMember::Operations)?.unwrap_or_else(|| default_operations(mode).to_vec());
        Ok(CreateCsrParams {
            registration_name: present(registration_name, CsrMember::RegistrationName)?,
            description: present(description, CsrMember::Description)?,
            information,
            dataset_id: present(dataset_id, CsrMember::DatasetId)?,
            tenant: present(tenant, CsrMember::Tenant)?,
            observation_interval: present(observation_interval, CsrMember::ObservationInterval)?,
            management_interval: present(management_interval, CsrMember::ManagementInterval)?,
            location: present(stored_geometry(location), CsrMember::Location)?,
            observation_space: present(stored_geometry(observation_space), CsrMember::ObservationSpace)?,
            operation_space: present(stored_geometry(operation_space), CsrMember::OperationSpace)?,
            expires_at: present(expires_at.map(NgsiLdDateTime::get), CsrMember::ExpiresAt)?,
            endpoint: required(endpoint, CsrMember::Endpoint)?,
            accept: present(accept, CsrMember::Accept)?,
            mode,
            operations,
            scope: present(scope, CsrMember::Scope)?,
            refresh_rate: present(refresh_rate, CsrMember::RefreshRate)?,
            management: present(management, CsrMember::Management)?,
            context_source_info: present(context_source_info, CsrMember::ContextSourceInfo)?,
            context_source_alias: present(context_source_alias, CsrMember::ContextSourceAlias)?,
            additional_properties,
        })
    }

    /// The fragment one Update Context Source Registration applies (clauses
    /// 5.9.3 and 5.5.8).
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationValidationReason::MemberMustNotBeNull`] for a
    /// `type`, `information`, `endpoint` or `mode` written as the NGSI-LD Null,
    /// which clause 5.5.8 does not let an update delete, and
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
        present(type_, CsrMember::Type)?;
        Ok(CsrUpdateFragment {
            registration_name,
            description,
            information: present(information, CsrMember::Information)?.map(non_empty_information).transpose()?,
            dataset_id,
            tenant,
            observation_interval,
            management_interval,
            location: stored_geometry(location),
            observation_space: stored_geometry(observation_space),
            operation_space: stored_geometry(operation_space),
            expires_at: expires_at.map(NgsiLdDateTime::get),
            endpoint: present(endpoint, CsrMember::Endpoint)?,
            accept,
            mode: present(mode, CsrMember::Mode)?,
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{registration_body::RegistrationBody, store::create_params::CreateCsrParams};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::rendering::instant_text};
    use andromeda_ngsi_ld_model::{clearable::Clearable, registration::operation::CsrOperation, request_body::RequestBody};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
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
        RegistrationBody::from_request_text(document.to_string().as_bytes(), CoreTermAliases::keyword_spellings())
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
            ("expiresAt", json!("2099-03-04T05:06:07.123456789Z")),
            ("expiresAt", json!("2099-03-04T05:06:07.1234567Z")),
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
            ("customThing", Value::Null),
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
        payload["expiresAt"] = json!("2099-03-04T05:06:07.123456Z");
        payload["scope"] = json!(["/Madrid/Gardens"]);
        payload["contextSourceAlias"] = json!("broker-alias-x");
        payload["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}});
        payload["customThing"] = json!({"kept": true});
        payload["status"] = json!("ok");
        let params = create_of(&payload).expect("valid registration");
        pretty_assertions::assert_eq!(params.dataset_id.as_ref().map(Vec::len), Some(2));
        assert!(params.management.is_some());
        pretty_assertions::assert_eq!(params.expires_at.map(instant_text), Some(String::from("2099-03-04T05:06:07.123456Z")));
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
            br#"{"type": "ContextSourceRegistration", "zeta": 1, "endpoint": "https://source.example.org", "alpha": {"b": [1.50]}, "information": [{"propertyNames": ["speed"]}]}"#, CoreTermAliases::keyword_spellings(),
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
            br#"{"@context": "https://example.org/ctx", "type": "ContextSourceRegistration", "endpoint": "https://source.example.org", "information": [{"propertyNames": ["speed"]}]}"#, CoreTermAliases::keyword_spellings(),
        )
        .expect("a body");
        assert!(body.additional_properties.is_empty());
    }

    /// Clauses 4.4 and 5.5.7: a registration spelling `type`, `id` and the
    /// type of an entity selector as `@type` and `@id`, or under the terms a
    /// context aliases them with, reads as the plain spelling.
    #[test]
    fn a_registration_spelled_with_keywords_or_aliases_reads_as_spelled_plainly() {
        let mut aliases = CoreTermAliases::keyword_spellings().clone(); // clone: the fixture extends the shared table
        aliases.alias("kind", "type");
        aliases.alias("ident", "id");
        let rendered = |text: &str, aliases: &CoreTermAliases| {
            format!(
                "{:?}",
                RegistrationBody::from_request_text(text.as_bytes(), aliases).expect("a registration body")
            )
        };
        let plain = rendered(
            r#"{"type": "ContextSourceRegistration", "id": "urn:ngsi-ld:ContextSourceRegistration:1", "endpoint": "https://source.example.org", "information": [{"entities": [{"type": "Vehicle"}]}]}"#,
            CoreTermAliases::keyword_spellings(),
        );
        let keywords = rendered(
            r#"{"@type": "ContextSourceRegistration", "@id": "urn:ngsi-ld:ContextSourceRegistration:1", "endpoint": "https://source.example.org", "information": [{"entities": [{"@type": "Vehicle"}]}]}"#,
            CoreTermAliases::keyword_spellings(),
        );
        let spelled_by_aliases = rendered(
            r#"{"kind": "ContextSourceRegistration", "ident": "urn:ngsi-ld:ContextSourceRegistration:1", "endpoint": "https://source.example.org", "information": [{"entities": [{"kind": "Vehicle"}]}]}"#,
            &aliases,
        );
        pretty_assertions::assert_eq!(keywords, plain);
        pretty_assertions::assert_eq!(spelled_by_aliases, plain);
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

    /// Every optional member of Table 5.2.9-1, beside whether its data type
    /// is an array.
    const DELETABLE_MEMBERS: [(&str, bool); 17] = [
        ("registrationName", false),
        ("description", false),
        ("datasetId", true),
        ("tenant", false),
        ("observationInterval", false),
        ("managementInterval", false),
        ("location", false),
        ("observationSpace", false),
        ("operationSpace", false),
        ("expiresAt", false),
        ("accept", false),
        ("operations", true),
        ("scope", true),
        ("refreshRate", false),
        ("management", false),
        ("contextSourceInfo", true),
        ("contextSourceAlias", false),
    ];

    /// Clause 5.2.1: a creation body refuses a Context Source Property written
    /// as the NGSI-LD Null.
    #[test]
    fn create_refuses_a_context_source_property_written_as_the_ngsi_ld_null() {
        let body = body_of(&with("custom", json!("urn:ngsi-ld:null"))).expect("a body");
        let refusal = body.refuse_ngsi_ld_null_properties().expect_err("the Null on create");
        assert!(refusal.to_string().contains("custom"), "{refusal}");
        assert!(
            body_of(&with("custom", json!({"kept": true})))
                .expect("a body")
                .refuse_ngsi_ld_null_properties()
                .is_ok()
        );
    }

    #[test]
    fn update_distinguishes_absent_deleted_and_present_members() {
        let fragment = body_of(&json!({"description": "urn:ngsi-ld:null", "management": {"timeout": 10}}))
            .expect("a body")
            .into_update_fragment()
            .expect("valid fragment");
        assert!(fragment.description.is_ngsi_ld_null());
        assert!(fragment.registration_name.is_absent());
        assert!(matches!(fragment.management, Clearable::Set(_)));
    }

    /// Clauses 4.5.0 and 5.5.8: the NGSI-LD Null deletes every optional member
    /// whatever its data type, an array member also as an array holding the
    /// Null alone.
    #[test]
    fn update_deletes_every_optional_member_written_as_the_ngsi_ld_null() {
        for (member, array) in DELETABLE_MEMBERS {
            let spellings = if array {
                vec![json!("urn:ngsi-ld:null"), json!(["urn:ngsi-ld:null"])]
            } else {
                vec![json!("urn:ngsi-ld:null")]
            };
            for spelling in spellings {
                let fragment = body_of(&json!({ member: spelling }))
                    .expect("a body")
                    .into_update_fragment()
                    .expect("valid fragment");
                assert!(!fragment.is_empty(), "{member} = {spelling}");
                pretty_assertions::assert_eq!(format!("{fragment:?}").matches("NgsiLdNull").count(), 1, "{member} = {spelling}");
            }
        }
    }

    /// Clause 5.2.1: no member admits a JSON `null`, on create or on update,
    /// a Context Source Property included.
    #[test]
    fn a_json_null_is_refused_on_every_member() {
        for (member, _) in DELETABLE_MEMBERS.into_iter().chain([
            ("type", false),
            ("information", true),
            ("endpoint", false),
            ("mode", false),
            ("id", false),
            ("custom", false),
        ]) {
            assert!(body_of(&json!({ member: null })).is_err(), "{member}");
            assert!(body_of(&with(member, Value::Null)).is_err(), "{member}");
        }
    }

    /// Clause 5.2.1: a creation body has nothing to delete, so the NGSI-LD Null
    /// is refused on every member.
    #[test]
    fn create_refuses_the_ngsi_ld_null_on_every_member() {
        for (member, _) in DELETABLE_MEMBERS {
            let refusal = create_of(&with(member, json!("urn:ngsi-ld:null"))).expect_err("the Null on create");
            assert!(refusal.contains("must not be the NGSI-LD Null"), "{member}: {refusal}");
        }
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

    /// Clause 5.5.8 lets an update delete optional members only: the members of
    /// cardinality 1 and the forwarding mode stay.
    #[test]
    fn update_refuses_to_delete_type_information_endpoint_or_mode() {
        for (member, spelling) in [
            ("type", json!("urn:ngsi-ld:null")),
            ("information", json!("urn:ngsi-ld:null")),
            ("information", json!(["urn:ngsi-ld:null"])),
            ("endpoint", json!("urn:ngsi-ld:null")),
            ("mode", json!("urn:ngsi-ld:null")),
        ] {
            let refused = body_of(&json!({ member: spelling }))
                .map_err(|error| error.to_string())
                .and_then(|body| body.into_update_fragment().map_err(|error| error.to_string()));
            assert!(refused.is_err(), "{member} = {spelling} must be rejected");
        }
    }
}
