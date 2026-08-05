//! Presentation logic for subscription responses.
//!
//! Handles stripping of system attributes, runtime notification fields,
//! and JSON-LD context from subscription and CSR subscription payloads
//! before they are sent as HTTP responses.

use crate::{core::error::AndromedaError, renderer::Renderer};
use andromeda_domain::sys_attrs::SysAttrsInclusion;
use serde_json::Value;

/// Flavor selector for [`SubscriptionRenderer`].
///
/// Local subscriptions and context-source subscriptions share the core
/// sys-attribute stripping logic but diverge on JSON-LD context removal
/// and notification runtime scrubbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionFlavor {
    /// Subscription backed by this broker -- local subscription.
    Local,
    /// Context source subscription federated to a remote broker.
    ContextSource,
}

/// Renders a subscription payload for HTTP response.
///
/// Conditionally strips system attributes (`createdAt`, `modifiedAt`),
/// defaulted runtime fields (`isActive` when `true`, `localOnly` when `false`),
/// and -- for the [`SubscriptionFlavor::ContextSource`] flavor --
/// `jsonldContext` plus notification runtime fields.
#[derive(Debug)]
pub struct SubscriptionRenderer {
    payload: Value,
    flavor: SubscriptionFlavor,
    sys_attrs: SysAttrsInclusion,
}

impl SubscriptionRenderer {
    /// Create a renderer for the given payload and flavor.
    pub const fn new(payload: Value, flavor: SubscriptionFlavor) -> SubscriptionRenderer {
        SubscriptionRenderer {
            payload,
            flavor,
            sys_attrs: SysAttrsInclusion::Strip,
        }
    }

    /// Set whether system attributes are included in the response.
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> Self {
        self.sys_attrs = sys_attrs;
        self
    }
}

impl Renderer for SubscriptionRenderer {
    type Output = Value;

    fn render(self) -> Result<Value, AndromedaError> {
        let SubscriptionRenderer {
            mut payload,
            flavor,
            sys_attrs,
        } = self;
        if sys_attrs == SysAttrsInclusion::Include {
            return Ok(payload);
        }
        let Some(obj) = payload.as_object_mut() else {
            return Ok(payload);
        };
        obj.remove("createdAt");
        obj.remove("modifiedAt");
        // isActive is only shown when explicitly false (paused); true is the default
        if obj.get("isActive").and_then(|v| v.as_bool()) == Some(true) {
            obj.remove("isActive");
        }
        match flavor {
            SubscriptionFlavor::Local => {
                // localOnly is only shown when true; false is the default
                if obj.get("localOnly").and_then(|v| v.as_bool()) != Some(true) {
                    obj.remove("localOnly");
                }
            }
            SubscriptionFlavor::ContextSource => {
                obj.remove("jsonldContext");
                if let Some(notification) = obj.get_mut("notification")
                    && let Some(notif_obj) = notification.as_object_mut()
                {
                    notif_obj.remove("timesSent");
                    notif_obj.remove("lastNotification");
                    notif_obj.remove("lastSuccess");
                    notif_obj.remove("lastFailure");
                }
            }
        }
        Ok(payload)
    }
}

/// Output of [`JsonldContextStripRenderer`]: the payload with `jsonldContext`
/// removed, together with the extracted context value.
#[derive(Debug)]
pub struct JsonldContextStripOutput {
    /// The payload with `jsonldContext` removed.
    pub payload: Value,
    /// The extracted `jsonldContext` value, if one was present.
    pub extracted: Option<Value>,
}

/// Extracts and removes the `jsonldContext` field from a subscription payload.
///
/// Used for subscription responses where the stored JSON-LD context needs to
/// be surfaced to the response context before being stripped from the body.
#[derive(Debug)]
pub struct JsonldContextStripRenderer {
    payload: Value,
}

impl JsonldContextStripRenderer {
    /// Create a renderer for the given payload.
    pub const fn new(payload: Value) -> JsonldContextStripRenderer {
        JsonldContextStripRenderer { payload }
    }
}

impl Renderer for JsonldContextStripRenderer {
    type Output = JsonldContextStripOutput;

    fn render(self) -> Result<JsonldContextStripOutput, AndromedaError> {
        let mut payload = self.payload;
        let extracted = payload.as_object_mut().and_then(|obj| obj.remove("jsonldContext"));
        Ok(JsonldContextStripOutput { payload, extracted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render_local(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        SubscriptionRenderer::new(value, SubscriptionFlavor::Local)
            .with_sys_attrs(sys_attrs)
            .render()
            .expect("render")
    }

    fn render_csource(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        SubscriptionRenderer::new(value, SubscriptionFlavor::ContextSource)
            .with_sys_attrs(sys_attrs)
            .render()
            .expect("render")
    }

    #[test]
    fn render_subscription_strips_sys_attrs_when_not_included() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "isActive": true,
            "localOnly": false,
            "entities": [{"type": "Vehicle"}]
        });
        let rendered = render_local(value, SysAttrsInclusion::Strip);
        assert!(rendered.get("createdAt").is_none());
        assert!(rendered.get("modifiedAt").is_none());
        assert!(rendered.get("isActive").is_none());
        assert!(rendered.get("localOnly").is_none());
        assert!(rendered.get("entities").is_some());
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
        let rendered = render_local(original.clone(), SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered, original);
    }

    #[test]
    fn render_subscription_keeps_is_active_when_false() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "isActive": false
        });
        let rendered = render_local(value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("isActive").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn render_subscription_keeps_local_only_when_true() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "localOnly": true
        });
        let rendered = render_local(value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("localOnly").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn render_csource_subscription_strips_sys_attrs_and_notification_runtime() {
        let value = json!({
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
        let rendered = render_csource(value, SysAttrsInclusion::Strip);
        assert!(rendered.get("createdAt").is_none());
        assert!(rendered.get("modifiedAt").is_none());
        assert!(rendered.get("jsonldContext").is_none());
        assert!(rendered.get("isActive").is_none());
        let notif = rendered.get("notification").expect("notification");
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
        let rendered = render_csource(original.clone(), SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered, original);
    }

    #[test]
    fn strip_jsonld_context_removes_and_returns() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "jsonldContext": "https://example.org/ctx"
        });
        let output = JsonldContextStripRenderer::new(value).render().expect("render");
        pretty_assertions::assert_eq!(output.extracted, Some(json!("https://example.org/ctx")));
        assert!(output.payload.get("jsonldContext").is_none());
    }

    #[test]
    fn strip_jsonld_context_returns_none_when_absent() {
        let value = json!({"id": "urn:ngsi-ld:Subscription:1"});
        let output = JsonldContextStripRenderer::new(value).render().expect("render");
        assert!(output.extracted.is_none());
    }
}
