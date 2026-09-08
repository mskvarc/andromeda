//! Extraction of context-source registration payload members into their types.
//!
//! Every clause 5.2.9 member is read through the model type that owns its
//! restriction — `NonEmptyText`, `TimeInterval`, `RegistrationManagementInfo`,
//! `DatasetSelector` — so a value outside Table 5.2.9-1 is a typed
//! deserialisation failure reported as `BadRequestData`, and nothing past this
//! module sees a `Value` for a member the specification has given a shape.

use crate::{
    error::RegistrationError,
    geometry::unwrap_geo_property_to_geometry,
    store::{CreateCsrParams, CsrUpdateFragment},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, non_empty_text::NonEmptyText, tenant::TenantName};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistrationType, CsrOperation, ForwardingMode, RegistrationInfo},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

/// Read one optional member into its type; absent stays `None`.
fn deserialize_member<T>(payload: &Value, key: &str) -> Result<Option<T>, RegistrationError>
where
    T: DeserializeOwned,
{
    payload
        .get(key)
        .map(|value| {
            serde_json::from_value(value.clone()) // clone: serde owns the selected member while the payload remains borrowed.
                .map_err(|error| RegistrationError::csr_validation(format!("Invalid '{key}' value: {error}")))
        })
        .transpose()
}

/// Read one mandatory member into its type (clause 5.2.9 cardinality 1).
fn deserialize_required<T>(payload: &Value, key: &str) -> Result<T, RegistrationError>
where
    T: DeserializeOwned,
{
    deserialize_member(payload, key)?.ok_or_else(|| RegistrationError::csr_validation(format!("{key} is required")))
}

/// Read one nullable member for an update: absent -> `None`, `null` ->
/// `Some(None)`, value -> `Some(Some(v))` (clause 5.5.8 member removal).
fn deserialize_nullable<T>(payload: &Value, key: &str) -> Result<Option<Option<T>>, RegistrationError>
where
    T: DeserializeOwned,
{
    match payload.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(Some(None)),
        Some(value) => serde_json::from_value(value.clone()) // clone: serde owns the selected member while the update payload remains borrowed.
            .map(|parsed| Some(Some(parsed)))
            .map_err(|error| RegistrationError::csr_validation(format!("Invalid '{key}' value: {error}"))),
    }
}

/// Read one optional `GeoJSON` geometry member, unwrapping a `GeoProperty` carrier.
fn deserialize_geometry(payload: &Value, key: &str) -> Result<Option<Geometry>, RegistrationError> {
    payload.get(key).map(unwrap_geo_property_to_geometry).transpose()
}

/// Read one nullable `GeoJSON` geometry member for an update.
fn deserialize_nullable_geometry(payload: &Value, key: &str) -> Result<Option<Option<Geometry>>, RegistrationError> {
    match payload.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(Some(None)),
        Some(value) => unwrap_geo_property_to_geometry(value).map(|geometry| Some(Some(geometry))),
    }
}

/// Parse a clause 4.6.3 `DateTime` member, naming the member in the error.
fn parse_datetime(value: &Value, key: &str) -> Result<DateTime<Utc>, RegistrationError> {
    let raw = value
        .as_str()
        .ok_or_else(|| RegistrationError::csr_validation(format!("'{key}' must be a string")))?;
    DateTime::parse_from_rfc3339(raw)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| RegistrationError::csr_validation(format!("Invalid '{key}' value '{raw}': {error}")))
}

/// Parse an ISO 8601 duration member, naming the member in the error.
pub(super) fn parse_duration_value(value: &Value, key: &str) -> Result<Iso8601Duration, RegistrationError> {
    let raw = value
        .as_str()
        .ok_or_else(|| RegistrationError::csr_validation(format!("'{key}' must be a string")))?;
    raw.parse::<Iso8601Duration>()
        .map_err(|error| RegistrationError::csr_validation(format!("Invalid '{key}' value '{raw}': {error:?}")))
}

/// Table 5.2.9-1: `information` is an array of at least one block; an empty
/// array is not allowed.
fn reject_empty_information(information: &[RegistrationInfo]) -> Result<(), RegistrationError> {
    if information.is_empty() {
        return Err(RegistrationError::csr_validation("information must not be an empty array"));
    }
    Ok(())
}

/// Clause 4.20: `federationOps` is the effective operation set when the
/// registration omits an explicit subset. The effective value is what gets
/// stored, so retrieval and routing read the same thing.
fn effective_operations(operations: Option<Vec<CsrOperation>>) -> Vec<CsrOperation> {
    operations.unwrap_or_else(|| vec![CsrOperation::FederationOps])
}

pub(super) fn extract_create_params(payload: &Value) -> Result<CreateCsrParams, RegistrationError> {
    // Table 5.2.9-1 gives `type`, `endpoint` and `information` cardinality 1.
    let _: ContextSourceRegistrationType = deserialize_required(payload, "type")?;
    let information = deserialize_required::<Vec<RegistrationInfo>>(payload, "information")?;
    reject_empty_information(&information)?;
    Ok(CreateCsrParams {
        registration_name: deserialize_member::<NonEmptyText>(payload, "registrationName")?,
        description: deserialize_member::<NonEmptyText>(payload, "description")?,
        information,
        dataset_id: deserialize_member::<Vec<DatasetSelector>>(payload, "datasetId")?,
        tenant: deserialize_member::<TenantName>(payload, "tenant")?,
        observation_interval: deserialize_member::<TimeInterval>(payload, "observationInterval")?,
        management_interval: deserialize_member::<TimeInterval>(payload, "managementInterval")?,
        location: deserialize_geometry(payload, "location")?,
        observation_space: deserialize_geometry(payload, "observationSpace")?,
        operation_space: deserialize_geometry(payload, "operationSpace")?,
        expires_at: payload.get("expiresAt").map(|value| parse_datetime(value, "expiresAt")).transpose()?,
        endpoint: deserialize_required::<Url>(payload, "endpoint")?,
        accept: deserialize_member::<MediaTypeBuf>(payload, "accept")?,
        mode: deserialize_member::<ForwardingMode>(payload, "mode")?.unwrap_or_default(),
        operations: effective_operations(deserialize_member::<Vec<CsrOperation>>(payload, "operations")?),
        scope: deserialize_member(payload, "scope")?,
        refresh_rate: payload.get("refreshRate").map(|value| parse_duration_value(value, "refreshRate")).transpose()?,
        management: deserialize_member::<RegistrationManagementInfo>(payload, "management")?,
        context_source_info: deserialize_member::<Vec<KeyValuePair>>(payload, "contextSourceInfo")?,
        jsonld_context: deserialize_member::<Url>(payload, "jsonldContext")?,
        context_source_alias: deserialize_member::<ContextSourcePseudonym>(payload, "contextSourceAlias")?,
        additional_properties: extract_additional_properties(payload),
    })
}

pub(super) fn extract_update_fragment(fragment: &Value) -> Result<CsrUpdateFragment, RegistrationError> {
    let information = deserialize_member::<Vec<RegistrationInfo>>(fragment, "information")?;
    if let Some(information) = information.as_deref() {
        reject_empty_information(information)?;
    }
    Ok(CsrUpdateFragment {
        registration_name: deserialize_nullable::<NonEmptyText>(fragment, "registrationName")?,
        description: deserialize_nullable::<NonEmptyText>(fragment, "description")?,
        information,
        dataset_id: deserialize_nullable::<Vec<DatasetSelector>>(fragment, "datasetId")?,
        tenant: deserialize_nullable::<TenantName>(fragment, "tenant")?,
        observation_interval: deserialize_nullable::<TimeInterval>(fragment, "observationInterval")?,
        management_interval: deserialize_nullable::<TimeInterval>(fragment, "managementInterval")?,
        location: deserialize_nullable_geometry(fragment, "location")?,
        observation_space: deserialize_nullable_geometry(fragment, "observationSpace")?,
        operation_space: deserialize_nullable_geometry(fragment, "operationSpace")?,
        expires_at: match fragment.get("expiresAt") {
            None => None,
            Some(value) if value.is_null() => Some(None),
            Some(value) => Some(Some(parse_datetime(value, "expiresAt")?)),
        },
        endpoint: deserialize_member::<Url>(fragment, "endpoint")?,
        accept: deserialize_nullable::<MediaTypeBuf>(fragment, "accept")?,
        mode: deserialize_member::<ForwardingMode>(fragment, "mode")?,
        operations: deserialize_nullable::<Vec<CsrOperation>>(fragment, "operations")?,
        scope: deserialize_nullable(fragment, "scope")?,
        refresh_rate: match fragment.get("refreshRate") {
            None => None,
            Some(value) if value.is_null() => Some(None),
            Some(value) => Some(Some(parse_duration_value(value, "refreshRate")?)),
        },
        management: deserialize_nullable::<RegistrationManagementInfo>(fragment, "management")?,
        context_source_info: deserialize_nullable::<Vec<KeyValuePair>>(fragment, "contextSourceInfo")?,
        jsonld_context: deserialize_nullable::<Url>(fragment, "jsonldContext")?,
        context_source_alias: deserialize_nullable::<ContextSourcePseudonym>(fragment, "contextSourceAlias")?,
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
    use crate::payload::{extract_create_params, extract_update_fragment};
    use andromeda_ngsi_ld_model::registration::CsrOperation;
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
        pretty_assertions::assert_eq!(fragment.description, Some(None));
        assert!(fragment.registration_name.is_none());
        assert!(fragment.management.as_ref().is_some_and(Option::is_some));
    }

    #[test]
    fn update_rejects_a_member_outside_its_restriction() {
        assert!(extract_update_fragment(&json!({"management": {"timeout": 0}})).is_err());
        assert!(extract_update_fragment(&json!({"description": ""})).is_err());
        assert!(extract_update_fragment(&json!({"information": [{"entities": []}]})).is_err());
        assert!(extract_update_fragment(&json!({"information": []})).is_err());
    }
}
