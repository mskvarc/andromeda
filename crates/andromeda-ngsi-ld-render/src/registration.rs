//! Presentation logic for context source registration (CSR) responses.
//!
//! Handles stripping of system attributes and runtime status fields from
//! CSR payloads before they are sent as HTTP responses.

use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::Value;

/// Renders a context source registration payload for HTTP response.
///
/// Strips the clause 4.8 system timestamps (`createdAt`, `modifiedAt`) unless
/// `sysAttrs` was requested. The registration status members (`status`,
/// `timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`) are read-only
/// auto-generated members (clause 5.2.9 Table 5.2.9-2) returned when known;
/// `sysAttrs` does not govern them, so they are never stripped.
#[derive(Debug)]
pub struct RegistrationRenderer {
    /// The registration payload to scrub.
    payload: Value,
    /// Whether the clause 4.8 system attributes survive.
    sys_attrs: SysAttrsInclusion,
}

impl RegistrationRenderer {
    /// Create a renderer for the given payload.
    pub const fn new(payload: Value) -> RegistrationRenderer {
        RegistrationRenderer {
            payload,
            sys_attrs: SysAttrsInclusion::Strip,
        }
    }

    /// Set whether system attributes are included in the response.
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> Self {
        self.sys_attrs = sys_attrs;
        self
    }

    /// Strip the members clause 4.8 withholds and return the payload.
    ///
    /// Removing members cannot fail, so this answers with the value rather
    /// than a `Result` no caller could act on.
    pub fn render(self) -> Value {
        let RegistrationRenderer { mut payload, sys_attrs } = self;
        let Some(obj) = payload.as_object_mut() else {
            return payload;
        };
        // Clause 4.8: `sysAttrs` governs only the system timestamps. The
        // registration status members are read-only auto-generated members
        // (clause 5.2.9 Table 5.2.9-2) returned when known regardless of
        // `sysAttrs`, so they are not stripped here.
        if sys_attrs == SysAttrsInclusion::Strip {
            obj.remove("createdAt");
            obj.remove("modifiedAt");
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        RegistrationRenderer::new(value).with_sys_attrs(sys_attrs).render()
    }

    #[test]
    fn render_registration_strips_only_the_timestamps_when_sys_attrs_not_included() {
        // Clause 4.8: `sysAttrs` withholds only createdAt/modifiedAt. The
        // read-only status members (clause 5.2.9 Table 5.2.9-2) are returned
        // when known, regardless.
        let value = json!({
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
        let rendered = render(value, SysAttrsInclusion::Strip);
        assert!(rendered.get("createdAt").is_none(), "{rendered}");
        assert!(rendered.get("modifiedAt").is_none(), "{rendered}");
        pretty_assertions::assert_eq!(rendered.get("status").and_then(Value::as_str), Some("ok"));
        pretty_assertions::assert_eq!(rendered.get("timesSent").and_then(Value::as_i64), Some(10));
        assert!(rendered.get("timesFailed").is_some(), "{rendered}");
        assert!(rendered.get("lastSuccess").is_some(), "{rendered}");
        assert!(rendered.get("lastFailure").is_some(), "{rendered}");
        assert!(rendered.get("endpoint").is_some(), "{rendered}");
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
        let rendered = render(original.clone(), SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered, original);
    }

    #[test]
    fn render_registration_handles_non_object() {
        let value = json!("not an object");
        let rendered = render(value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered, json!("not an object"));
    }
}
