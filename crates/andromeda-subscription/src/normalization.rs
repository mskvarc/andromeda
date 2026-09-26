//! Subscription persistence normalization.

use andromeda_jsonld::compaction::NameCompactor;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    notification::DEFAULT_NOTIFICATION_TRIGGERS,
    subscription::{SubscriptionFilters, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{
    filter::{
        compaction::{compact_q_expr, revert_expand_values},
        expression::Expr,
    },
    projection::term::ProjectionTerm,
};
use chrono::{DateTime, Utc};
use serde_json::{Number, Value};
use std::collections::HashSet;

/// Render `timeInterval` and `throttling` as JSON integers whenever they hold
/// a whole number of seconds.
///
/// Both members round-trip through the store as floating-point numbers, so a
/// subscription written with `5` would otherwise be read back as `5.0`.
pub fn fixup_integer_fields(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        for key in ["timeInterval", "throttling"] {
            if let Some(seconds) = object.get(key).and_then(Value::as_f64)
                && let Some(whole) = whole_seconds(seconds)
            {
                object.insert(key.to_owned(), Value::Number(Number::from(whole)));
            }
        }
    }
}

/// The whole-second value of `seconds`, or `None` when it carries a fraction
/// or falls outside the range an integer JSON number can hold.
///
/// The conversion goes through the decimal rendering rather than a cast so
/// that a value too large for [`i64`] keeps its floating-point form instead of
/// being truncated into range.
fn whole_seconds(seconds: f64) -> Option<i64> {
    if seconds.fract() != 0.0 {
        return None;
    }
    format!("{seconds:.0}").parse().ok()
}

/// Render the clause 5.2.12 default `notificationTrigger` a Subscription that
/// named none is matched under.
///
/// Table 5.2.12-1: "If not present, the default is the combination
/// attributeCreated and attributeUpdated". What the Context Subscriber wrote is
/// what the broker stores, so the default is supplied when the Subscription is
/// served rather than when it is created -- and it is supplied here, on the
/// Subscription of clause 5.2.12, not on the Context Source Registration
/// Subscription of clause 5.11, whose notifications carry a `triggerReason`
/// (clause 5.3.2) that no notification trigger selects.
pub fn fixup_default_notification_trigger(value: &mut Value) {
    if value.get("notificationTrigger").is_none()
        && let Some(obj) = value.as_object_mut()
    {
        let defaults = DEFAULT_NOTIFICATION_TRIGGERS.map(|trigger| trigger.to_string()).to_vec();
        obj.insert("notificationTrigger".to_string(), serde_json::json!(defaults));
    }
}

/// Parse stringified jsonldContext back to structured JSON.
pub fn fixup_jsonld_context(value: &mut Value) {
    let parsed = value.get("jsonldContext").and_then(|v| v.as_str()).and_then(|s| {
        let trimmed = s.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            serde_json::from_str::<Value>(trimmed).ok()
        } else {
            None
        }
    });
    if let Some(parsed) = parsed
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("jsonldContext".to_string(), parsed);
    }
}

/// Report the clause 5.2.12 `status` as `expired` once `expiresAt` has passed.
///
/// Clause 5.8.1.4 has the status change "automatically" when the expiration
/// date is due. The sweeper that persists the transition runs on a cadence,
/// so a read that lands between the due instant and the sweep reports the
/// status the subscription already has rather than the one the row still
/// carries. `isActive` is the client's own member and is left as written.
pub fn fixup_expired_status(value: &mut Value, expires_at: Option<DateTime<Utc>>) {
    if let Some(expires_at) = expires_at
        && expires_at <= Utc::now()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("status".to_string(), Value::String(SubscriptionStatus::Expired.to_string()));
    }
}

/// Which names of an attribute list are left as written rather than compacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralKeywords {
    /// Every name is an attribute IRI and compacts.
    Compact,
    /// The clause 4.21 `id`, `type` and `scope` name structural members of
    /// the representation, not attributes, and stay as written.
    Keep,
}

/// Compact every string of one attribute-name list in place.
async fn compact_name_list(compactor: &NameCompactor<'_>, names: &mut [Value], keywords: StructuralKeywords) {
    for name in names.iter_mut() {
        let Some(iri) = name.as_str().map(String::from) else {
            continue;
        };
        if keywords == StructuralKeywords::Keep
            && matches!(
                iri.as_str(),
                ProjectionTerm::ID_KEYWORD | ProjectionTerm::TYPE_KEYWORD | ProjectionTerm::SCOPE_KEYWORD
            )
        {
            continue;
        }
        *name = Value::String(compactor.compact_attr_iri(&iri).await);
    }
}

/// Compact IRI-bearing fields in a serialized Subscription for HTTP response.
pub async fn compact_subscription_iris(compactor: &NameCompactor<'_>, value: &mut Value) {
    // Compact entities[].type IRIs
    if let Some(entities) = value.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_iri) = entity_sel.get("type").and_then(|v| v.as_str()).map(String::from) {
                let compacted = compactor.compact_type_iri(&type_iri).await;
                if let Some(obj) = entity_sel.as_object_mut() {
                    obj.insert("type".to_string(), Value::String(compacted));
                }
            }
        }
    }

    if let Some(watched) = value.get_mut("watchedAttributes").and_then(Value::as_array_mut) {
        compact_name_list(compactor, watched, StructuralKeywords::Compact).await;
    }

    // Clause 5.2.14: `attributes` names attributes only; `pick` and `omit`
    // may also name the structural members of clause 4.21.
    if let Some(notification) = value.get_mut("notification") {
        for (member, keywords) in [
            ("attributes", StructuralKeywords::Compact),
            ("pick", StructuralKeywords::Keep),
            ("omit", StructuralKeywords::Keep),
        ] {
            if let Some(names) = notification.get_mut(member).and_then(Value::as_array_mut) {
                compact_name_list(compactor, names, keywords).await;
            }
        }
    }
}

/// Spell the stored `q` and `csf` of a serialized Subscription in the terms
/// of the reader's `@context` (clause 5.5.7).
///
/// Both are clause 4.9 expressions whose attribute terms were expanded when
/// the subscription was written; the serialized document carries them as
/// their expanded text. `scopeQ` is a clause 4.19 scope path and carries no
/// attribute terms, so it is rendered as stored.
///
/// The `q` operands `expandValues` coerced are put back too. Table 5.2.12-1
/// makes that coercion something done "prior to executing the query", so it
/// belongs to the filter a match runs and not to the Subscription resource: a
/// client that created `q=status=="active"` with `expandValues=status` reads
/// back the filter it wrote. `csf` is exempt because `expandValues` names
/// Entity attributes, while `csf` addresses a registration's Context Source
/// Properties.
pub async fn compact_subscription_filters(compactor: &NameCompactor<'_>, value: &mut Value, filters: &SubscriptionFilters) {
    if let Some(expr) = filters.q.as_ref() {
        let opted_in: HashSet<AttributeName> = filters.expand_values.iter().flatten().cloned().collect(); // clone: the opt-in list stays on the stored resource
        // clone: the stored filter stays in the resource while the compacted copy is rendered
        let reverted = revert_expand_values(expr.clone(), &opted_in, compactor).await;
        insert_compacted(value, "q", &compact_q_expr(reverted, compactor).await);
    }
    if let Some(expr) = filters.csf.as_ref() {
        // clone: the stored filter stays in the resource while the compacted copy is rendered
        insert_compacted(value, "csf", &compact_q_expr(expr.clone(), compactor).await);
    }
    for (member, names) in [("expandValues", filters.expand_values.as_deref()), ("jsonKeys", filters.json_keys.as_deref())] {
        let Some(names) = names else {
            continue;
        };
        let compacted = compact_opt_in_names(compactor, names).await;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(member.to_string(), Value::String(compacted));
        }
    }
}

/// Write one compacted filter expression back into the serialized document.
fn insert_compacted(value: &mut Value, member: &str, expr: &Expr) {
    if let Some(object) = value.as_object_mut() {
        object.insert(member.to_owned(), Value::String(expr.to_string()));
    }
}

/// Spell one stored `expandValues` / `jsonKeys` list in the reader's terms,
/// in the comma-separated form Table 5.2.12-1 defines.
async fn compact_opt_in_names(compactor: &NameCompactor<'_>, names: &[AttributeName]) -> String {
    let mut compacted = Vec::with_capacity(names.len());
    for name in names {
        compacted.push(compactor.compact_attr_iri(name.as_str()).await);
    }
    compacted.join(",")
}

#[cfg(test)]
mod tests {
    use crate::normalization::{
        compact_subscription_filters,
        compact_subscription_iris,
        fixup_default_notification_trigger,
        fixup_expired_status,
        fixup_integer_fields,
    };
    use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::subscription::SubscriptionFilters;
    use chrono::{Duration, Utc};
    use serde_json::json;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[test]
    fn integral_delivery_intervals_serialize_as_json_integers() {
        let mut value = serde_json::json!({"timeInterval": 5.0, "throttling": 2.0});
        fixup_integer_fields(&mut value);
        assert_eq!(value, serde_json::json!({"timeInterval": 5, "throttling": 2}));
    }

    #[test]
    fn absent_notification_trigger_receives_the_clause_default() {
        let mut value = serde_json::json!({});
        fixup_default_notification_trigger(&mut value);
        assert_eq!(value["notificationTrigger"], serde_json::json!(["attributeCreated", "attributeUpdated"]));
    }

    /// Clause 5.8.1.4: a subscription whose `expiresAt` is due reports
    /// `expired` on read, whatever status the row still carries, and the
    /// client's `isActive` is not rewritten.
    #[test]
    fn a_due_expiration_reports_status_expired_at_read_time() {
        let mut value = json!({"status": "active", "isActive": true});
        fixup_expired_status(&mut value, Some(Utc::now() - Duration::seconds(1)));
        pretty_assertions::assert_eq!(value, json!({"status": "expired", "isActive": true}));
    }

    #[test]
    fn a_future_or_absent_expiration_leaves_the_status_alone() {
        let mut future = json!({"status": "active"});
        fixup_expired_status(&mut future, Some(Utc::now() + Duration::hours(1)));
        pretty_assertions::assert_eq!(future, json!({"status": "active"}));

        let mut perpetual = json!({"status": "paused"});
        fixup_expired_status(&mut perpetual, None);
        pretty_assertions::assert_eq!(perpetual, json!({"status": "paused"}));
    }

    /// Clause 5.5.7: a stored `q` and `csf` read back in the reader's terms.
    #[tokio::test]
    async fn stored_filters_are_compacted_into_the_readers_terms() {
        let engine = offline_engine();
        let filters: SubscriptionFilters = serde_json::from_value(json!({
            "q": "\"https://uri.etsi.org/ngsi-ld/default-context/speed\">10",
            "csf": "\"https://uri.etsi.org/ngsi-ld/default-context/tier\"==\"gold\""
        }))
        .expect("valid filters");
        let mut value = serde_json::to_value(&filters).expect("serialises");

        compact_subscription_filters(&NameCompactor::new(&engine, &ContextSpec::none()), &mut value, &filters).await;

        pretty_assertions::assert_eq!(value["q"], json!("speed>10"));
        pretty_assertions::assert_eq!(value["csf"], json!("tier==\"gold\""));
    }

    /// Table 5.2.12-1: `expandValues` and `jsonKeys` are stored expanded and
    /// read back in the reader's terms, still as one comma-separated String.
    #[tokio::test]
    async fn the_stored_opt_in_lists_are_compacted_into_the_readers_terms() {
        let engine = offline_engine();
        let filters: SubscriptionFilters = serde_json::from_value(json!({
            "expandValues": "https://uri.etsi.org/ngsi-ld/default-context/speed,https://uri.etsi.org/ngsi-ld/default-context/colour",
            "jsonKeys": "https://uri.etsi.org/ngsi-ld/default-context/payload"
        }))
        .expect("valid filters");
        let mut value = serde_json::to_value(&filters).expect("serialises");

        compact_subscription_filters(&NameCompactor::new(&engine, &ContextSpec::none()), &mut value, &filters).await;

        pretty_assertions::assert_eq!(value["expandValues"], json!("speed,colour"));
        pretty_assertions::assert_eq!(value["jsonKeys"], json!("payload"));
    }

    /// Clause 4.21: the `pick`/`omit` keywords are structural members and
    /// come back as written beside the compacted attribute IRIs.
    #[tokio::test]
    async fn pick_and_omit_compact_their_attribute_iris_and_keep_the_keywords() {
        let engine = offline_engine();
        let mut value = json!({
            "notification": {
                "pick": ["id", "https://uri.etsi.org/ngsi-ld/default-context/speed"],
                "omit": ["scope", "https://uri.etsi.org/ngsi-ld/default-context/colour"]
            }
        });

        compact_subscription_iris(&NameCompactor::new(&engine, &ContextSpec::none()), &mut value).await;

        pretty_assertions::assert_eq!(value["notification"]["pick"], json!(["id", "speed"]));
        pretty_assertions::assert_eq!(value["notification"]["omit"], json!(["scope", "colour"]));
    }
}
