//! Extraction of context-source registration payload members into their types.
//!
//! Every clause 5.2.9 member is read through the model type that owns its
//! restriction — `NonEmptyText`, `TimeInterval`, `RegistrationManagementInfo`,
//! `DatasetSelector` — so a value outside Table 5.2.9-1 is a typed
//! deserialisation failure reported as `BadRequestData`, and nothing past this
//! module sees a `Value` for a member the specification has given a shape.

use crate::{
    csr_member::CsrMember,
    error::RegistrationError,
    geometry::unwrap_geo_property_to_geometry,
    mode_rules::default_operations,
    nullable_member::NullableMember,
    registration_validation_reason::RegistrationValidationReason,
    store::{create_params::CreateCsrParams, update_fragment::CsrUpdateFragment},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    non_empty_text::NonEmptyText,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistrationType, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Read one optional member into its type; absent stays `None`.
fn deserialize_member<T>(payload: &Value, key: CsrMember) -> Result<Option<T>, RegistrationError>
where
    T: DeserializeOwned,
{
    payload
        .get(key.key())
        .map(|value| {
            serde_json::from_value(value.clone()) // clone: serde owns the selected member while the payload remains borrowed.
                .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::MemberValue { member: key, source }))
        })
        .transpose()
}

/// Read one mandatory member into its type (clause 5.2.9 cardinality 1).
fn deserialize_required<T>(payload: &Value, key: CsrMember) -> Result<T, RegistrationError>
where
    T: DeserializeOwned,
{
    deserialize_member(payload, key)?.ok_or_else(|| RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member: key }))
}

/// Read one nullable member for an update (clause 5.5.8 member removal).
fn deserialize_nullable<T>(payload: &Value, key: CsrMember) -> Result<NullableMember<T>, RegistrationError>
where
    T: DeserializeOwned,
{
    match payload.get(key.key()) {
        None => Ok(NullableMember::Absent),
        Some(value) if value.is_null() => Ok(NullableMember::Cleared),
        Some(value) => serde_json::from_value(value.clone()) // clone: serde owns the selected member while the update payload remains borrowed.
            .map(NullableMember::Set)
            .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::MemberValue { member: key, source })),
    }
}

/// Read one optional `GeoJSON` geometry member, unwrapping a `GeoProperty` carrier.
fn deserialize_geometry(payload: &Value, key: CsrMember) -> Result<Option<Geometry>, RegistrationError> {
    payload.get(key.key()).map(unwrap_geo_property_to_geometry).transpose()
}

/// Read one nullable `GeoJSON` geometry member for an update.
fn deserialize_nullable_geometry(payload: &Value, key: CsrMember) -> Result<NullableMember<Geometry>, RegistrationError> {
    match payload.get(key.key()) {
        None => Ok(NullableMember::Absent),
        Some(value) if value.is_null() => Ok(NullableMember::Cleared),
        Some(value) => unwrap_geo_property_to_geometry(value).map(NullableMember::Set),
    }
}

/// Parse a clause 4.6.3 `DateTime` member, naming the member in the error.
fn parse_datetime(value: &Value, key: CsrMember) -> Result<DateTime<Utc>, RegistrationError> {
    let raw = value
        .as_str()
        .ok_or_else(|| RegistrationError::Validation(RegistrationValidationReason::MemberNotString { member: key }))?;
    DateTime::parse_from_rfc3339(raw)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::DateTimeMember { member: key, source }))
}

/// Parse an ISO 8601 duration member, naming the member in the error.
pub(super) fn parse_duration_value(value: &Value, key: CsrMember) -> Result<Iso8601Duration, RegistrationError> {
    let raw = value
        .as_str()
        .ok_or_else(|| RegistrationError::Validation(RegistrationValidationReason::MemberNotString { member: key }))?;
    raw.parse::<Iso8601Duration>()
        .map_err(|_| RegistrationError::Validation(RegistrationValidationReason::DurationMember { member: key }))
}

/// Table 5.2.9-1: `information` is an array of at least one block; an empty
/// array is not allowed.
const fn reject_empty_information(information: &[RegistrationInfo]) -> Result<(), RegistrationError> {
    if information.is_empty() {
        return Err(RegistrationError::Validation(RegistrationValidationReason::InformationMustNotBeEmpty));
    }
    Ok(())
}

pub(super) fn extract_create_params(payload: &Value) -> Result<CreateCsrParams, RegistrationError> {
    // Table 5.2.9-1 gives `type`, `endpoint` and `information` cardinality 1.
    let _: ContextSourceRegistrationType = deserialize_required(payload, CsrMember::Type)?;
    let information = deserialize_required::<Vec<RegistrationInfo>>(payload, CsrMember::Information)?;
    reject_empty_information(&information)?;
    // Table 5.2.9-1: absent `mode` is `inclusive`. The mode is read before
    // `operations` because it decides which default set applies to it.
    let mode = deserialize_member::<ForwardingMode>(payload, CsrMember::Mode)?.unwrap_or_default();
    // The effective value is what gets stored, so retrieval and routing read
    // the same thing the admission check saw.
    let operations = deserialize_member::<Vec<CsrOperation>>(payload, CsrMember::Operations)?.unwrap_or_else(|| default_operations(mode).to_vec());
    Ok(CreateCsrParams {
        registration_name: deserialize_member::<NonEmptyText>(payload, CsrMember::RegistrationName)?,
        description: deserialize_member::<NonEmptyText>(payload, CsrMember::Description)?,
        information,
        dataset_id: deserialize_member::<Vec<DatasetSelector>>(payload, CsrMember::DatasetId)?,
        tenant: deserialize_member::<TenantName>(payload, CsrMember::Tenant)?,
        observation_interval: deserialize_member::<TimeInterval>(payload, CsrMember::ObservationInterval)?,
        management_interval: deserialize_member::<TimeInterval>(payload, CsrMember::ManagementInterval)?,
        location: deserialize_geometry(payload, CsrMember::Location)?,
        observation_space: deserialize_geometry(payload, CsrMember::ObservationSpace)?,
        operation_space: deserialize_geometry(payload, CsrMember::OperationSpace)?,
        expires_at: payload
            .get(CsrMember::ExpiresAt.key())
            .map(|value| parse_datetime(value, CsrMember::ExpiresAt))
            .transpose()?,
        endpoint: deserialize_required::<DereferenceableUri>(payload, CsrMember::Endpoint)?,
        accept: deserialize_member::<MediaTypeBuf>(payload, CsrMember::Accept)?,
        mode,
        operations,
        scope: deserialize_member(payload, CsrMember::Scope)?,
        refresh_rate: payload
            .get(CsrMember::RefreshRate.key())
            .map(|value| parse_duration_value(value, CsrMember::RefreshRate))
            .transpose()?,
        management: deserialize_member::<RegistrationManagementInfo>(payload, CsrMember::Management)?,
        context_source_info: deserialize_member::<Vec<KeyValuePair>>(payload, CsrMember::ContextSourceInfo)?,
        jsonld_context: deserialize_member::<DereferenceableUri>(payload, CsrMember::JsonldContext)?,
        context_source_alias: deserialize_member::<ContextSourcePseudonym>(payload, CsrMember::ContextSourceAlias)?,
        additional_properties: extract_additional_properties(payload),
    })
}

pub(super) fn extract_update_fragment(fragment: &Value) -> Result<CsrUpdateFragment, RegistrationError> {
    let information = deserialize_member::<Vec<RegistrationInfo>>(fragment, CsrMember::Information)?;
    if let Some(information) = information.as_deref() {
        reject_empty_information(information)?;
    }
    Ok(CsrUpdateFragment {
        registration_name: deserialize_nullable::<NonEmptyText>(fragment, CsrMember::RegistrationName)?,
        description: deserialize_nullable::<NonEmptyText>(fragment, CsrMember::Description)?,
        information,
        dataset_id: deserialize_nullable::<Vec<DatasetSelector>>(fragment, CsrMember::DatasetId)?,
        tenant: deserialize_nullable::<TenantName>(fragment, CsrMember::Tenant)?,
        observation_interval: deserialize_nullable::<TimeInterval>(fragment, CsrMember::ObservationInterval)?,
        management_interval: deserialize_nullable::<TimeInterval>(fragment, CsrMember::ManagementInterval)?,
        location: deserialize_nullable_geometry(fragment, CsrMember::Location)?,
        observation_space: deserialize_nullable_geometry(fragment, CsrMember::ObservationSpace)?,
        operation_space: deserialize_nullable_geometry(fragment, CsrMember::OperationSpace)?,
        expires_at: match fragment.get(CsrMember::ExpiresAt.key()) {
            None => NullableMember::Absent,
            Some(value) if value.is_null() => NullableMember::Cleared,
            Some(value) => NullableMember::Set(parse_datetime(value, CsrMember::ExpiresAt)?),
        },
        endpoint: deserialize_member::<DereferenceableUri>(fragment, CsrMember::Endpoint)?,
        accept: deserialize_nullable::<MediaTypeBuf>(fragment, CsrMember::Accept)?,
        mode: deserialize_member::<ForwardingMode>(fragment, CsrMember::Mode)?,
        operations: deserialize_nullable::<Vec<CsrOperation>>(fragment, CsrMember::Operations)?,
        scope: deserialize_nullable(fragment, CsrMember::Scope)?,
        refresh_rate: match fragment.get(CsrMember::RefreshRate.key()) {
            None => NullableMember::Absent,
            Some(value) if value.is_null() => NullableMember::Cleared,
            Some(value) => NullableMember::Set(parse_duration_value(value, CsrMember::RefreshRate)?),
        },
        management: deserialize_nullable::<RegistrationManagementInfo>(fragment, CsrMember::Management)?,
        context_source_info: deserialize_nullable::<Vec<KeyValuePair>>(fragment, CsrMember::ContextSourceInfo)?,
        jsonld_context: deserialize_nullable::<DereferenceableUri>(fragment, CsrMember::JsonldContext)?,
        context_source_alias: deserialize_nullable::<ContextSourcePseudonym>(fragment, CsrMember::ContextSourceAlias)?,
        additional_properties: extract_additional_properties(fragment),
    })
}

pub(super) fn extract_additional_properties(payload: &Value) -> Option<Value> {
    let obj = payload.as_object()?;
    let additional: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(key, _)| !CreateCsrParams::STANDARD_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone())) // clone: owned key-value pairs for the additional properties map
        .collect();
    if additional.is_empty() { None } else { Some(Value::Object(additional)) }
}

#[cfg(test)]
mod tests {
    use crate::{
        nullable_member::NullableMember,
        payload::{extract_create_params, extract_update_fragment},
    };
    use andromeda_ngsi_ld_model::registration::operation::CsrOperation;
    use serde_json::json;

    fn baseline() -> serde_json::Value {
        json!({
            "type": "ContextSourceRegistration",
            "endpoint": "https://source.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}]
        })
    }

    fn with(member: &str, value: serde_json::Value) -> serde_json::Value {
        let mut payload = baseline();
        payload[member] = value;
        payload
    }

    #[test]
    fn create_materialises_the_clause_4_20_default_operation_group() {
        let params = extract_create_params(&baseline()).expect("valid registration");
        pretty_assertions::assert_eq!(params.operations, vec![CsrOperation::FederationOps]);
    }

    #[test]
    fn create_preserves_an_explicit_operation_subset() {
        let params = extract_create_params(&with("operations", json!(["retrieveEntity", "queryEntity"]))).expect("valid registration");
        pretty_assertions::assert_eq!(params.operations, vec![CsrOperation::RetrieveEntity, CsrOperation::QueryEntity]);
    }

    #[test]
    fn create_requires_type_endpoint_and_information() {
        for member in ["type", "endpoint", "information"] {
            let mut payload = baseline();
            payload.as_object_mut().expect("object").remove(member);
            assert!(extract_create_params(&payload).is_err(), "missing {member} must be rejected");
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
        ] {
            assert!(
                extract_create_params(&with(member, value.clone())).is_err(),
                "{member} = {value} must be rejected"
            ); // clone: reported on failure
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
        payload["customThing"] = json!({"kept": true});
        let params = extract_create_params(&payload).expect("valid registration");
        pretty_assertions::assert_eq!(params.dataset_id.as_ref().map(Vec::len), Some(2));
        assert!(params.management.is_some());
        assert!(params.management_interval.is_some_and(|interval| interval.end_at.is_none()));
        pretty_assertions::assert_eq!(params.additional_properties, Some(json!({"customThing": {"kept": true}})));
    }

    #[test]
    fn update_distinguishes_absent_null_and_present_members() {
        let fragment = extract_update_fragment(&json!({"description": null, "management": {"timeout": 10}})).expect("valid fragment");
        assert!(matches!(fragment.description, NullableMember::Cleared));
        assert!(matches!(fragment.registration_name, NullableMember::Absent));
        assert!(matches!(fragment.management, NullableMember::Set(_)));
    }

    #[test]
    fn update_rejects_a_member_outside_its_restriction() {
        assert!(extract_update_fragment(&json!({"management": {"timeout": 0}})).is_err());
        assert!(extract_update_fragment(&json!({"description": ""})).is_err());
        assert!(extract_update_fragment(&json!({"information": [{"entities": []}]})).is_err());
        assert!(extract_update_fragment(&json!({"information": []})).is_err());
    }
}
