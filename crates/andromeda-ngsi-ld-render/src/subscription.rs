//! Presentation logic for subscription responses.
//!
//! Handles stripping of system attributes and defaulted members from
//! subscription and CSR subscription payloads before HTTP responses. The
//! `jsonldContext` member of a subscription is part of the clause 5.2.12
//! representation and stays in the retrieved document (clause 5.8.3.5).

use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
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
/// Strips the clause 4.8 system timestamps (`createdAt`, `modifiedAt`) unless
/// `sysAttrs` was requested. Independently of `sysAttrs`, it always hides
/// defaulted fields (`isActive` when `true`, `localOnly` when `false`) and, for
/// the [`SubscriptionFlavor::ContextSource`] flavor, always removes the internal
/// `jsonldContext`. Clause 5.2.14.2 notification runtime members remain in
/// query and retrieve representations regardless of `sysAttrs`.
#[derive(Debug)]
pub struct SubscriptionRenderer {
    /// The subscription payload to scrub.
    payload: Value,
    /// Which of the two subscription resources this is.
    flavor: SubscriptionFlavor,
    /// Whether the clause 4.8 system attributes survive.
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

    /// Strip the members clause 4.8 withholds and return the payload.
    ///
    /// Removing members cannot fail, so this answers with the value rather
    /// than a `Result` no caller could act on.
    pub fn render(self) -> Value {
        let SubscriptionRenderer {
            mut payload,
            flavor,
            sys_attrs,
        } = self;
        let Some(obj) = payload.as_object_mut() else {
            return payload;
        };
        // Clause 4.8: `sysAttrs` governs only the system timestamps. Every other
        // policy below — default-value hiding, `jsonldContext` removal, CSR
        // notification-runtime scrubbing — is independent of it, so `sysAttrs`
        // must not resurrect an internal or defaulted field.
        if sys_attrs == SysAttrsInclusion::Strip {
            obj.remove("createdAt");
            obj.remove("modifiedAt");
        }
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
            }
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render_local(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        SubscriptionRenderer::new(value, SubscriptionFlavor::Local).with_sys_attrs(sys_attrs).render()
    }

    fn render_csource(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        SubscriptionRenderer::new(value, SubscriptionFlavor::ContextSource)
            .with_sys_attrs(sys_attrs)
            .render()
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
    fn render_subscription_keeps_timestamps_but_still_hides_defaults_when_sys_attrs_included() {
        // Clause 4.8: `sysAttrs` keeps the timestamps, but default-value hiding
        // is a separate policy it does not switch off — an `isActive: true` or
        // `localOnly: false` default is still omitted.
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "isActive": true,
            "localOnly": false
        });
        let rendered = render_local(value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered.get("createdAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(rendered.get("modifiedAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        assert!(rendered.get("isActive").is_none(), "the isActive default is still hidden: {rendered}");
        assert!(rendered.get("localOnly").is_none(), "the localOnly default is still hidden: {rendered}");
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
    fn render_csource_subscription_strips_sys_attrs_and_keeps_notification_runtime() {
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
        pretty_assertions::assert_eq!(notif.get("timesSent").and_then(Value::as_i64), Some(42));
        assert!(notif.get("lastNotification").is_some());
        assert!(notif.get("lastSuccess").is_some());
        assert!(notif.get("lastFailure").is_some());
    }

    #[test]
    fn render_csource_subscription_keeps_system_and_notification_timestamps_when_sys_attrs_included() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "createdAt": "2025-01-01T00:00:00Z",
            "modifiedAt": "2025-01-01T00:00:00Z",
            "jsonldContext": "https://example.org/ctx",
            "notification": {
                "endpoint": {"uri": "https://example.org"},
                "timesSent": 42,
                "lastNotification": "2025-01-01T00:00:00Z"
            }
        });
        let rendered = render_csource(value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered.get("createdAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(rendered.get("modifiedAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        assert!(rendered.get("jsonldContext").is_none(), "jsonldContext must never leak in the body: {rendered}");
        let notif = rendered.get("notification").expect("notification");
        assert!(notif.get("endpoint").is_some(), "the endpoint stays: {rendered}");
        pretty_assertions::assert_eq!(notif.get("timesSent").and_then(Value::as_i64), Some(42));
        assert!(
            notif.get("lastNotification").is_some(),
            "notification runtime is output-only, not internal: {rendered}"
        );
    }

    /// Clause 5.2.12: `jsonldContext` is a member of the subscription and
    /// survives rendering of a local subscription, with or without `sysAttrs`.
    #[test]
    fn render_subscription_keeps_jsonld_context_in_the_document() {
        for sys_attrs in [SysAttrsInclusion::Strip, SysAttrsInclusion::Include] {
            let value = json!({
                "id": "urn:ngsi-ld:Subscription:1",
                "type": "Subscription",
                "jsonldContext": "https://example.org/ctx"
            });
            let rendered = render_local(value, sys_attrs);
            pretty_assertions::assert_eq!(rendered["jsonldContext"], json!("https://example.org/ctx"), "{sys_attrs:?}");
        }
    }
}
