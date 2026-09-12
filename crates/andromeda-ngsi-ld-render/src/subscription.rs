//! What a subscription representation withholds (ETSI GS CIM 009 clauses 4.8,
//! 5.2.12 and 5.2.14.2).
//!
//! Two members are generated with a default and are written back only when
//! they differ from it -- `isActive`, which Table 5.2.12-2 derives from
//! `status`, and `localOnly`, whose absence means `false`. The clause 4.8
//! system timestamps are withheld unless `sysAttrs` asks for them. Everything
//! else clause 5.2.12 defines stays, `jsonldContext` included: clause 5.8.3.5
//! returns "the subscription details as mandated by clause 5.2.12", and clause
//! 5.11.4.5 says the same of a Context Source Registration Subscription, whose
//! resource clause 5.11.2.3 takes from that very data type.
//!
//! The two resources are therefore rendered by one rule. They were not: the
//! context source half removed `jsonldContext` as an internal member, which
//! clause 5.8.1.4 contradicts by requiring it to be initialised from the
//! @context the subscription was created under, and which this module's own
//! documentation contradicted in the same breath.

use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::Value;

/// Renders a subscription payload for an HTTP response.
#[derive(Debug)]
pub struct SubscriptionRenderer {
    /// The subscription payload to scrub.
    payload: Value,
    /// Whether the clause 4.8 system attributes survive.
    sys_attrs: SysAttrsInclusion,
}

impl SubscriptionRenderer {
    /// Create a renderer for the given payload.
    #[must_use]
    pub const fn new(payload: Value) -> SubscriptionRenderer {
        SubscriptionRenderer {
            payload,
            sys_attrs: SysAttrsInclusion::Strip,
        }
    }

    /// Set whether system attributes are included in the response.
    #[must_use]
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> SubscriptionRenderer {
        self.sys_attrs = sys_attrs;
        self
    }

    /// Strip the members a subscription representation withholds and return
    /// the payload.
    ///
    /// Removing members cannot fail, so this answers with the value rather
    /// than a `Result` no caller could act on.
    #[must_use]
    pub fn render(self) -> Value {
        let SubscriptionRenderer { mut payload, sys_attrs } = self;
        let Some(obj) = payload.as_object_mut() else {
            return payload;
        };
        // Clause 4.8: `sysAttrs` governs only the system timestamps. Hiding a
        // member that equals its default is a separate policy it does not
        // switch off, so `sysAttrs` must not resurrect a defaulted field.
        if sys_attrs == SysAttrsInclusion::Strip {
            obj.remove("createdAt");
            obj.remove("modifiedAt");
        }
        // Table 5.2.12-2 derives `isActive` from `status`; it is written back
        // only when it is `false`, which is the paused subscription.
        if obj.get("isActive").and_then(Value::as_bool) == Some(true) {
            obj.remove("isActive");
        }
        // Table 5.2.12-1 defaults `localOnly` to false.
        if obj.get("localOnly").and_then(Value::as_bool) != Some(true) {
            obj.remove("localOnly");
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(value: Value, sys_attrs: SysAttrsInclusion) -> Value {
        SubscriptionRenderer::new(value).with_sys_attrs(sys_attrs).render()
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
        let rendered = render(value, SysAttrsInclusion::Strip);
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
        let rendered = render(value, SysAttrsInclusion::Include);
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
        let rendered = render(value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("isActive").and_then(serde_json::Value::as_bool), Some(false));
    }

    #[test]
    fn render_subscription_keeps_local_only_when_true() {
        let value = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "localOnly": true
        });
        let rendered = render(value, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(rendered.get("localOnly").and_then(serde_json::Value::as_bool), Some(true));
    }

    #[test]
    fn a_csource_subscription_keeps_its_jsonld_context_and_notification_runtime() {
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
        let rendered = render(value, SysAttrsInclusion::Strip);
        assert!(rendered.get("createdAt").is_none());
        assert!(rendered.get("modifiedAt").is_none());
        pretty_assertions::assert_eq!(rendered.get("jsonldContext").and_then(Value::as_str), Some("https://example.org/ctx"));
        assert!(rendered.get("isActive").is_none());
        let notif = rendered.get("notification").expect("notification");
        assert!(notif.get("endpoint").is_some());
        pretty_assertions::assert_eq!(notif.get("timesSent").and_then(Value::as_i64), Some(42));
        assert!(notif.get("lastNotification").is_some());
        assert!(notif.get("lastSuccess").is_some());
        assert!(notif.get("lastFailure").is_some());
    }

    #[test]
    fn a_csource_subscription_keeps_every_timestamp_when_sys_attrs_is_asked_for() {
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
        let rendered = render(value, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(rendered.get("createdAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(rendered.get("modifiedAt").and_then(Value::as_str), Some("2025-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(
            rendered.get("jsonldContext").and_then(Value::as_str),
            Some("https://example.org/ctx"),
            "clause 5.11.4.5 returns the clause 5.2.12 details, `jsonldContext` among them: {rendered}"
        );
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
            let rendered = render(value, sys_attrs);
            pretty_assertions::assert_eq!(rendered["jsonldContext"], json!("https://example.org/ctx"), "{sys_attrs:?}");
        }
    }
}
