//! Presentation logic for subscription responses.
//!
//! Handles stripping of system attributes, runtime notification fields,
//! and JSON-LD context from subscription and CSR subscription payloads
//! before they are sent as HTTP responses.

use crate::renderer::SysAttrsInclusion;
use serde_json::Value;

/// Render a subscription for response by conditionally stripping system attributes.
///
/// When `sys_attrs` is [`SysAttrsInclusion::Strip`], removes `createdAt`,
/// `modifiedAt`, and conditionally removes `isActive` (when true, since true
/// is the default) and `localOnly` (when not true, since false is the default).
///
/// Does NOT remove `jsonldContext` -- use [`strip_jsonld_context`] separately
/// when needed.
pub fn render_subscription(value: &mut Value, sys_attrs: SysAttrsInclusion) {
    if sys_attrs == SysAttrsInclusion::Include {
        return;
    }
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    obj.remove("createdAt");
    obj.remove("modifiedAt");
    // isActive is only shown when explicitly false (paused); true is the default
    if obj.get("isActive").and_then(|v| v.as_bool()) == Some(true) {
        obj.remove("isActive");
    }
    // localOnly is only shown when true; false is the default
    if obj.get("localOnly").and_then(|v| v.as_bool()) != Some(true) {
        obj.remove("localOnly");
    }
}

/// Render a context source subscription for response by conditionally stripping
/// system attributes, JSON-LD context, and notification runtime fields.
///
/// When `sys_attrs` is [`SysAttrsInclusion::Strip`], removes `createdAt`,
/// `modifiedAt`, `jsonldContext`, conditionally `isActive` (when true), and
/// notification runtime sub-fields (`timesSent`, `lastNotification`,
/// `lastSuccess`, `lastFailure`).
pub fn render_csource_subscription(value: &mut Value, sys_attrs: SysAttrsInclusion) {
    if sys_attrs == SysAttrsInclusion::Include {
        return;
    }
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    obj.remove("createdAt");
    obj.remove("modifiedAt");
    obj.remove("jsonldContext");
    // isActive is only shown when explicitly false (paused); true is the default
    if obj.get("isActive").and_then(|v| v.as_bool()) == Some(true) {
        obj.remove("isActive");
    }
    if let Some(notification) = obj.get_mut("notification")
        && let Some(notif_obj) = notification.as_object_mut()
    {
        notif_obj.remove("timesSent");
        notif_obj.remove("lastNotification");
        notif_obj.remove("lastSuccess");
        notif_obj.remove("lastFailure");
    }
}

/// Remove the `jsonldContext` field from a value and return it.
///
/// Used for subscription responses where the stored JSON-LD context needs to
/// be extracted to set the response context before being stripped from the body.
pub fn strip_jsonld_context(value: &mut Value) -> Option<Value> {
    value.as_object_mut().and_then(|obj| obj.remove("jsonldContext"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn render_subscription_strips_sys_attrs_when_not_included() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "isActive": true,
            "localOnly": false,
            "entities": [{"type": "Vehicle"}]
        });
        render_subscription(&mut value, SysAttrsInclusion::Strip);
        assert!(value.get("createdAt").is_none());
        assert!(value.get("modifiedAt").is_none());
        assert!(value.get("isActive").is_none());
        assert!(value.get("localOnly").is_none());
        assert!(value.get("entities").is_some());
    }

    #[test]
    fn render_subscription_preserves_all_when_sys_attrs_included() {
        let original = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "isActive": true,
            "localOnly": false
        });
        let mut value = original.clone();
        render_subscription(&mut value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(value, original);
    }

    #[test]
    fn render_subscription_keeps_is_active_when_false() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "isActive": false
        });
        render_subscription(&mut value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(value.get("isActive").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn render_subscription_keeps_local_only_when_true() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "localOnly": true
        });
        render_subscription(&mut value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(value.get("localOnly").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn render_csource_subscription_strips_sys_attrs_and_notification_runtime() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "jsonldContext": "https://example.org/ctx",
            "isActive": true,
            "notification": {
                "endpoint": {"uri": "https://example.org"},
                "timesSent": 42,
                "lastNotification": "2025-01-01T00:00:00Z",
                "lastSuccess": "2025-01-01T00:00:00Z",
                "lastFailure": "2025-01-01T00:00:00Z"
            }
        });
        render_csource_subscription(&mut value, SysAttrsInclusion::Strip);
        assert!(value.get("createdAt").is_none());
        assert!(value.get("modifiedAt").is_none());
        assert!(value.get("jsonldContext").is_none());
        assert!(value.get("isActive").is_none());
        let notif = value.get("notification").expect("notification");
        assert!(notif.get("endpoint").is_some());
        assert!(notif.get("timesSent").is_none());
        assert!(notif.get("lastNotification").is_none());
        assert!(notif.get("lastSuccess").is_none());
        assert!(notif.get("lastFailure").is_none());
    }

    #[test]
    fn render_csource_subscription_preserves_all_when_sys_attrs_included() {
        let original = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "notification": {"timesSent": 42}
        });
        let mut value = original.clone();
        render_csource_subscription(&mut value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(value, original);
    }

    #[test]
    fn strip_jsonld_context_removes_and_returns() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "jsonldContext": "https://example.org/ctx"
        });
        let ctx = strip_jsonld_context(&mut value);
        pretty_assertions::assert_eq!(ctx, Some(json!("https://example.org/ctx")));
        assert!(value.get("jsonldContext").is_none());
    }

    #[test]
    fn strip_jsonld_context_returns_none_when_absent() {
        let mut value = json!({"id": "urn:ngsi-ld:Subscription:1"});
        let ctx = strip_jsonld_context(&mut value);
        assert!(ctx.is_none());
    }
}
