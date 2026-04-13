//! Presentation logic for context source registration (CSR) responses.
//!
//! Handles stripping of system attributes and runtime status fields from
//! CSR payloads before they are sent as HTTP responses.

use crate::renderer::SysAttrsInclusion;
use serde_json::Value;

/// Render a context source registration for response by conditionally
/// stripping system attributes and runtime status fields.
///
/// When `sys_attrs` is [`SysAttrsInclusion::Strip`], removes `status`,
/// `timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`, `createdAt`,
/// and `modifiedAt`.
pub fn render_registration(value: &mut Value, sys_attrs: SysAttrsInclusion) {
    if sys_attrs == SysAttrsInclusion::Include {
        return;
    }
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    obj.remove("status");
    obj.remove("timesSent");
    obj.remove("timesFailed");
    obj.remove("lastSuccess");
    obj.remove("lastFailure");
    obj.remove("createdAt");
    obj.remove("modifiedAt");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn render_registration_strips_sys_attrs_when_not_included() {
        let mut value = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "status": "ok",
            "timesSent": 10,
            "timesFailed": 2,
            "lastSuccess": "2025-01-01T00:00:00Z",
            "lastFailure": "2025-01-01T00:00:00Z",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "endpoint": "https://example.org"
        });
        render_registration(&mut value, SysAttrsInclusion::Strip);
        assert!(value.get("status").is_none());
        assert!(value.get("timesSent").is_none());
        assert!(value.get("timesFailed").is_none());
        assert!(value.get("lastSuccess").is_none());
        assert!(value.get("lastFailure").is_none());
        assert!(value.get("createdAt").is_none());
        assert!(value.get("modifiedAt").is_none());
        assert!(value.get("endpoint").is_some());
    }

    #[test]
    fn render_registration_preserves_all_when_sys_attrs_included() {
        let original = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "status": "ok",
            "timesSent": 10,
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z"
        });
        let mut value = original.clone();
        render_registration(&mut value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(value, original);
    }

    #[test]
    fn render_registration_handles_non_object() {
        let mut value = json!("not an object");
        render_registration(&mut value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(value, json!("not an object"));
    }
}
