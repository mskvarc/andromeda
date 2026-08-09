//! Validation shared by context-source registration creation and update.

use crate::error::AndromedaError;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub(super) fn validate_common_members(payload: &Value) -> Result<(), AndromedaError> {
    if let Some(type_value) = payload.get("type")
        && type_value.as_str() != Some("ContextSourceRegistration")
    {
        return Err(AndromedaError::csr_validation("type must be 'ContextSourceRegistration'"));
    }

    if let Some(raw) = payload.get("expiresAt").and_then(Value::as_str) {
        let expires_at = DateTime::parse_from_rfc3339(raw).map_err(|_| AndromedaError::csr_validation("expiresAt must be a valid ISO 8601 date-time"))?;
        if expires_at <= Utc::now() {
            return Err(AndromedaError::csr_validation("expiresAt must be in the future"));
        }
    }

    if let Some(raw) = payload.get("mode").and_then(Value::as_str) {
        raw.parse::<ForwardingMode>()
            .map_err(|_| AndromedaError::csr_validation("mode must be one of: inclusive, exclusive, redirect, auxiliary"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::service::csource_registration::validation::validate_common_members;

    #[test]
    fn common_validation_rejects_an_unknown_forwarding_mode() {
        assert!(validate_common_members(&serde_json::json!({"mode": "unknown"})).is_err());
    }

    #[test]
    fn common_validation_accepts_each_specified_forwarding_mode() {
        for mode in ["inclusive", "exclusive", "redirect", "auxiliary"] {
            assert!(validate_common_members(&serde_json::json!({"mode": mode})).is_ok());
        }
    }
}
