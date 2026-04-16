//! Presentation logic for context source registration (CSR) responses.
//!
//! Handles stripping of system attributes and runtime status fields from
//! CSR payloads before they are sent as HTTP responses.

use crate::{
    core::error::AndromedaError,
    renderer::{Renderer, SysAttrsInclusion},
};
use serde_json::Value;

/// Renders a context source registration payload for HTTP response.
///
/// When [`SysAttrsInclusion::Strip`] is configured, removes `status`,
/// `timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`, `createdAt`,
/// and `modifiedAt`.
#[derive(Debug)]
pub struct RegistrationRenderer {
    payload: Value,
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
}

impl Renderer for RegistrationRenderer {
    type Output = Value;

    fn render(self) -> Result<Value, AndromedaError> {
        let RegistrationRenderer { mut payload, sys_attrs } = self;
        if sys_attrs == SysAttrsInclusion::Include {
            return Ok(payload);
        }
        let Some(obj) = payload.as_object_mut() else {
            return Ok(payload);
        };
        obj.remove("status");
        obj.remove("timesSent");
        obj.remove("timesFailed");
        obj.remove("lastSuccess");
        obj.remove("lastFailure");
        obj.remove("createdAt");
        obj.remove("modifiedAt");
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn render(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        RegistrationRenderer::new(value).with_sys_attrs(sys_attrs).render().expect("render")
    }

    #[test]
    fn render_registration_strips_sys_attrs_when_not_included() {
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
        assert!(rendered.get("status").is_none());
        assert!(rendered.get("timesSent").is_none());
        assert!(rendered.get("timesFailed").is_none());
        assert!(rendered.get("lastSuccess").is_none());
        assert!(rendered.get("lastFailure").is_none());
        assert!(rendered.get("createdAt").is_none());
        assert!(rendered.get("modifiedAt").is_none());
        assert!(rendered.get("endpoint").is_some());
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
