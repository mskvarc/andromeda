//! Extraction and validation of context-source registration payload fields.

use crate::{
    error::AndromedaError,
    service::{
        csource_extract::unwrap_geo_property_to_geometry,
        csource_registration::store::{CreateCsrParams, CsrUpdateFragment},
    },
};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant::TenantName};
use andromeda_ngsi_ld_model::registration::{CsrOperation, ForwardingMode, RegistrationInfo};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

fn deserialize_member<T>(payload: &Value, key: &str) -> Result<Option<T>, AndromedaError>
where
    T: DeserializeOwned,
{
    payload
        .get(key)
        .map(|value| {
            serde_json::from_value(value.clone()) // clone: serde owns the selected member while the payload remains borrowed.
                .map_err(|error| AndromedaError::csr_validation(format!("Invalid '{key}' value: {error}")))
        })
        .transpose()
}

pub(super) fn extract_create_params(payload: &Value) -> Result<CreateCsrParams<'_>, AndromedaError> {
    Ok(CreateCsrParams {
        registration_name: payload.get("registrationName").and_then(|v| v.as_str()),
        description: payload.get("description").and_then(|v| v.as_str()),
        information: deserialize_member::<Vec<RegistrationInfo>>(payload, "information")?,
        tenant: deserialize_member::<TenantName>(payload, "tenant")?,
        observation_interval: payload.get("observationInterval"),
        management_interval: payload.get("managementInterval"),
        location: payload
            .get("location")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        observation_space: payload
            .get("observationSpace")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        operation_space: payload
            .get("operationSpace")
            .map(crate::service::csource_extract::unwrap_geo_property_to_geometry)
            .transpose()?,
        expires_at: payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .map(|s| {
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'expiresAt' value '{}': {}", s, e)))
            })
            .transpose()?,
        endpoint: deserialize_member::<Url>(payload, "endpoint")?,
        accept: deserialize_member::<MediaTypeBuf>(payload, "accept")?,
        mode: payload
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<ForwardingMode>())
            .transpose()
            .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'mode' value: {}", e)))?
            .unwrap_or_default(),
        // Clause 4.20 defines federationOps as the effective set when the
        // registration omits an explicit subset. Persisting that effective
        // value keeps retrieval and routing semantics identical.
        operations: Some(deserialize_member::<Vec<CsrOperation>>(payload, "operations")?.unwrap_or_else(|| vec![CsrOperation::FederationOps])),
        scope: deserialize_member(payload, "scope")?,
        refresh_rate: payload.get("refreshRate").map(|value| parse_duration_value(value, "refreshRate")).transpose()?,
        management: payload.get("management"),
        context_source_info: payload.get("contextSourceInfo"),
        jsonld_context: deserialize_member::<Url>(payload, "jsonldContext")?,
        context_source_alias: deserialize_member::<ContextSourcePseudonym>(payload, "contextSourceAlias")?,
        additional_properties: extract_additional_properties(payload),
    })
}

pub(super) fn extract_update_fragment(fragment: &Value) -> Result<CsrUpdateFragment, AndromedaError> {
    Ok(CsrUpdateFragment {
        registration_name: extract_optional_string(fragment, "registrationName"),
        description: extract_optional_string(fragment, "description"),
        information: deserialize_member::<Vec<RegistrationInfo>>(fragment, "information")?,
        tenant: extract_optional_tenant_name(fragment, "tenant")?,
        observation_interval: extract_optional_value(fragment, "observationInterval"),
        management_interval: extract_optional_value(fragment, "managementInterval"),
        location: fragment
            .get("location")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        observation_space: fragment
            .get("observationSpace")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        operation_space: fragment
            .get("operationSpace")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    unwrap_geo_property_to_geometry(v).map(Some)
                }
            })
            .transpose()?,
        expires_at: fragment
            .get("expiresAt")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    v.as_str()
                        .ok_or_else(|| AndromedaError::csr_validation("'expiresAt' must be a string"))
                        .and_then(|s| {
                            DateTime::parse_from_rfc3339(s)
                                .map(|dt| Some(dt.with_timezone(&Utc)))
                                .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'expiresAt' value '{}': {}", s, e)))
                        })
                }
            })
            .transpose()?,
        endpoint: deserialize_member::<Url>(fragment, "endpoint")?,
        accept: extract_optional_typed::<MediaTypeBuf>(fragment, "accept")?,
        mode: fragment
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<ForwardingMode>())
            .transpose()
            .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'mode' value: {}", e)))?,
        operations: fragment
            .get("operations")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone()) // clone: serde_json::from_value consumes the Value
                        .map(Some)
                        .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'operations' value: {}", e)))
                }
            })
            .transpose()?,
        scope: fragment
            .get("scope")
            .map(|v| {
                if v.is_null() {
                    Ok(None)
                } else {
                    serde_json::from_value(v.clone()) // clone: serde_json::from_value consumes the Value
                        .map(Some)
                        .map_err(|e| AndromedaError::csr_validation(format!("Invalid 'scope' value: {}", e)))
                }
            })
            .transpose()?,
        refresh_rate: extract_optional_duration(fragment, "refreshRate")?,
        management: extract_optional_value(fragment, "management"),
        context_source_info: extract_optional_value(fragment, "contextSourceInfo"),
        jsonld_context: extract_optional_typed::<Url>(fragment, "jsonldContext")?,
        context_source_alias: extract_optional_typed::<ContextSourcePseudonym>(fragment, "contextSourceAlias")?,
        additional_properties: extract_additional_properties(fragment),
    })
}

/// For nullable string fields: absent -> None, null -> Some(None), value -> Some(Some(s))
pub(super) fn extract_optional_string(payload: &Value, key: &str) -> Option<Option<String>> {
    payload.get(key).map(|v| if v.is_null() { None } else { v.as_str().map(|s| s.to_string()) })
}

fn extract_optional_typed<T>(payload: &Value, key: &str) -> Result<Option<Option<T>>, AndromedaError>
where
    T: DeserializeOwned,
{
    match payload.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(Some(None)),
        Some(value) => serde_json::from_value(value.clone()) // clone: serde owns the selected member while the update payload remains borrowed.
            .map(|parsed| Some(Some(parsed)))
            .map_err(|error| AndromedaError::csr_validation(format!("Invalid '{key}' value: {error}"))),
    }
}

pub(super) fn parse_duration_value(value: &Value, key: &str) -> Result<Iso8601Duration, AndromedaError> {
    let raw = value
        .as_str()
        .ok_or_else(|| AndromedaError::csr_validation(format!("'{key}' must be a string")))?;
    raw.parse::<Iso8601Duration>()
        .map_err(|error| AndromedaError::csr_validation(format!("Invalid '{key}' value '{raw}': {error:?}")))
}

pub(super) fn extract_optional_duration(payload: &Value, key: &str) -> Result<Option<Option<Iso8601Duration>>, AndromedaError> {
    match payload.get(key) {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(Some(None)),
        Some(value) => Ok(Some(Some(parse_duration_value(value, key)?))),
    }
}

/// For nullable Value fields: absent -> None, null -> Some(None), value -> Some(Some(v))
pub(super) fn extract_optional_value(payload: &Value, key: &str) -> Option<Option<Value>> {
    payload.get(key).map(|v| if v.is_null() { None } else { Some(v.clone()) }) // clone: need owned Value; v is borrowed from payload
}

pub(super) fn extract_optional_tenant_name(payload: &Value, key: &str) -> Result<Option<Option<TenantName>>, AndromedaError> {
    let value = payload.get(key);
    let result = match value {
        None => None,
        Some(raw) if raw.is_null() => Some(None),
        Some(raw) => {
            let tenant = raw
                .as_str()
                .ok_or_else(|| AndromedaError::csr_validation(format!("'{key}' must be a string")))?;
            let tenant_name =
                TenantName::try_from(tenant.to_string()).map_err(|_| AndromedaError::csr_validation(format!("'{key}' must be a non-empty string")))?;
            Some(Some(tenant_name))
        }
    };
    Ok(result)
}

pub(super) fn extract_additional_properties(payload: &Value) -> Option<Value> {
    let obj = payload.as_object()?;
    let mut additional = serde_json::Map::new();
    for (key, value) in obj {
        if !CreateCsrParams::STANDARD_KEYS.contains(&key.as_str()) {
            additional.insert(key.clone(), value.clone()); // clone: owned key-value pairs for new additional properties Map
        }
    }
    if additional.is_empty() { None } else { Some(Value::Object(additional)) }
}

#[cfg(test)]
mod tests {
    use crate::service::csource_registration::payload::extract_create_params;
    use andromeda_ngsi_ld_model::registration::CsrOperation;
    use serde_json::json;

    #[test]
    fn create_materialises_the_clause_4_20_default_operation_group() {
        let payload = json!({"endpoint": "https://source.example.org"});
        let params = extract_create_params(&payload).expect("valid registration");

        pretty_assertions::assert_eq!(params.operations, Some(vec![CsrOperation::FederationOps]));
    }

    #[test]
    fn create_preserves_an_explicit_operation_subset() {
        let payload = json!({"endpoint": "https://source.example.org", "operations": ["retrieveEntity", "queryEntity"]});
        let params = extract_create_params(&payload).expect("valid registration");

        pretty_assertions::assert_eq!(params.operations, Some(vec![CsrOperation::RetrieveEntity, CsrOperation::QueryEntity]));
    }
}
