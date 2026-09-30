//! Compilation of persisted subscriptions into matching state.

use crate::{
    compiled::{CompiledCsrSubscription, CompiledSubscription},
    geo_filter::SubscriptionGeoFilter,
    stored_subscription::StoredSubscription,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_model::{
    notification::effective_notification_triggers,
    subscription::{EntitySelector, NotificationParams, Subscription},
};
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    projection::{compiled::Projection, term::ProjectionTerm},
    temporal::window::TimeInterval,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use std::{collections::HashSet, num::NonZeroU32};
use tracing::debug;

/// Compile a stored subscription into a pre-compiled form for fast matching.
///
/// Takes the stored pair rather than the resource alone: matching ends in a
/// delivery enqueued against the surrogate key, so the key has to survive
/// compilation alongside everything the match itself needs.
pub fn compile_subscription(stored: &StoredSubscription) -> CompiledSubscription {
    let sub = &stored.subscription;
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    let triggers = effective_notification_triggers(sub.selection.notification_trigger.as_deref());
    let watched_attrs = sub.selection.watched_attributes.clone().map(HashSet::from_iter);
    CompiledSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        key: stored.key,
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.url().clone(), // clone: compiled state owns the URL every delivery is built from.
        endpoint_accept: sub.notification.endpoint.accept.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_cooldown: sub.notification.endpoint.cooldown,
        endpoint_timeout: sub.notification.endpoint.timeout,
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        notification_attributes: sub.notification.attributes.clone(), // clone: compiled state owns its projection.
        notification_projection: notification_projection(&sub.notification, sub.id.as_ref()),
        notification_format: sub.notification.format,
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        expires_at: sub.lifecycle.expires_at,
        created_at: sub.lifecycle.created_at,
        compiled_q: sub.filters.q.clone(), // clone: compiled state owns its filter AST.
        compiled_geo_q: SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref()),
        compiled_scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        dataset_id: sub.filters.dataset_id.clone(),    // clone: compiled state owns dataset identifiers.
        notification_join: sub.notification.join,
        // Clause 4.5.23: `joinLevel` defaults to one hop and is otherwise
        // honoured exactly. Zero never reaches here -- the model rejects it at
        // deserialization.
        notification_join_level: sub.notification.join_level.unwrap_or(NonZeroU32::MIN),
        compiled_csf: sub.filters.csf.clone(), // clone: compiled state owns its filter AST.
        local_only: sub.delivery.local_only.unwrap_or(false),
        ngsild_conformance: sub.delivery.ngsild_conformance,
    }
}

/// Compile a context-source subscription for in-memory registration matching
/// (clauses 5.11.2.4 and 5.12).
#[must_use]
pub fn compile_csource_subscription(sub: &Subscription) -> CompiledCsrSubscription {
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    CompiledCsrSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.url().clone(), // clone: compiled state owns the URL every delivery is built from.
        endpoint_accept: sub.notification.endpoint.accept.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_cooldown: sub.notification.endpoint.cooldown,
        endpoint_timeout: sub.notification.endpoint.timeout,
        selectors,
        matching_attrs: csource_matching_attributes(sub.selection.watched_attributes.as_deref(), sub.notification.attributes.as_deref()),
        temporal_q: sub.filters.temporal_q.as_ref().and_then(|value| compile_temporal_q(value, sub.id.as_ref())),
        geo_q: SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref()),
        scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        expires_at: sub.lifecycle.expires_at,
        compiled_csf: sub.filters.csf.clone(), // clone: compiled state owns its filter AST.
    }
}

/// The attribute identifiers a context-source subscription matches with
/// (clauses 5.11.7 and 5.12): the combination of `watchedAttributes` and
/// `notification.attributes`, without repetition. Empty, when neither names an
/// attribute, stands for every attribute.
fn csource_matching_attributes(watched: Option<&[AttributeName]>, notified: Option<&[AttributeName]>) -> Vec<AttributeName> {
    let mut combined: Vec<AttributeName> = Vec::new();
    for name in watched.into_iter().flatten().chain(notified.into_iter().flatten()) {
        if !combined.contains(name) {
            combined.push(name.clone()); // clone: compiled state owns the combined names
        }
    }
    combined
}

/// Pre-parse a stored `temporalQ` (clause 5.2.21) into the interval filter a
/// registration's `observationInterval` or `managementInterval` is matched
/// against (clause 5.11.2.4).
///
/// `None` when the subscription carries no usable `temporalQ`, in which case
/// it is matched as one without a temporal query.
pub fn compile_temporal_q(temporal_q: &serde_json::Value, sub_id: &str) -> Option<CompiledCsrTemporalFilter> {
    let object = temporal_q.as_object()?;
    let timerel = match object.get("timerel").and_then(serde_json::Value::as_str).map(str::parse::<TimeRel>) {
        Some(Ok(timerel)) => Some(timerel),
        Some(Err(error)) => {
            debug!(subscription = sub_id, %error, "Invalid subscription temporalQ timerel");
            return None;
        }
        None => None,
    };
    let time_at = match object.get("timeAt").and_then(serde_json::Value::as_str).map(DateTime::parse_from_rfc3339) {
        Some(Ok(instant)) => Some(instant.with_timezone(&Utc)),
        Some(Err(error)) => {
            debug!(subscription = sub_id, %error, "Invalid subscription temporalQ timeAt");
            return None;
        }
        None => None,
    };
    let end_time_at = match object.get("endTimeAt").and_then(serde_json::Value::as_str).map(DateTime::parse_from_rfc3339) {
        Some(Ok(instant)) => Some(instant.with_timezone(&Utc)),
        Some(Err(error)) => {
            debug!(subscription = sub_id, %error, "Invalid subscription temporalQ endTimeAt");
            return None;
        }
        None => None,
    };
    let timeproperty = match object.get("timeproperty").and_then(serde_json::Value::as_str).map(str::parse::<TimeProp>) {
        Some(Ok(timeproperty)) => Some(timeproperty),
        Some(Err(error)) => {
            debug!(subscription = sub_id, %error, "Invalid subscription temporalQ timeproperty");
            return None;
        }
        None => None,
    };
    let interval = match TimeInterval::from_parts(timerel, time_at, end_time_at) {
        Ok(interval) => interval,
        Err(error) => {
            debug!(subscription = sub_id, %error, "Invalid subscription temporalQ window");
            return None;
        }
    };
    Some(CompiledCsrTemporalFilter::builder().interval(interval).timeproperty(timeproperty).build())
}

/// Compile the clause 5.2.14 `pick` and `omit` members into the projection a
/// query's `pick`/`omit` parameters compile to (clause 4.21).
///
/// The stored names were expanded through the subscription's `@context` when
/// the resource was written (clause 5.5.7), except the three clause 4.21
/// keywords, which name structural members of the representation rather than
/// attributes and are kept as written.
#[must_use]
pub fn notification_projection(notification: &NotificationParams, sub_id: &str) -> Projection {
    Projection::builder()
        .pick(projection_terms(notification.pick.as_deref().unwrap_or_default(), sub_id))
        .omit(projection_terms(notification.omit.as_deref().unwrap_or_default(), sub_id))
        .build()
}

/// The compiled terms of one stored `pick` or `omit` list.
///
/// A name that is neither a keyword nor an absolute IRI can address nothing in
/// an expanded entity, so it is left out of the projection rather than carried
/// as a term that never matches.
fn projection_terms(names: &[AttributeName], sub_id: &str) -> Vec<ProjectionTerm> {
    names
        .iter()
        .filter_map(|name| match name.as_str() {
            ProjectionTerm::ID_KEYWORD => Some(ProjectionTerm::Id),
            ProjectionTerm::TYPE_KEYWORD => Some(ProjectionTerm::Type),
            ProjectionTerm::SCOPE_KEYWORD => Some(ProjectionTerm::Scope),
            // `&str` is unbounded, so a catch-all is the only way to close
            // this match; anything that is not one of the three keywords is
            // an attribute term.
            other => match IriBuf::new(other.to_owned()) {
                Ok(iri) => Some(ProjectionTerm::Attribute { iri, linked: Vec::new() }),
                Err(error) => {
                    debug!(subscription = sub_id, name = other, %error, "Subscription projection member is not an expanded attribute name");
                    None
                }
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::compilation::{compile_temporal_q, csource_matching_attributes, notification_projection};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::subscription::NotificationParams;
    use andromeda_ngsi_ld_query::{projection::term::ProjectionTerm, temporal::window::TimeInterval};
    use iri_rs::IriBuf;

    fn notification(value: serde_json::Value) -> NotificationParams {
        serde_json::from_value(value).expect("valid notification parameters")
    }

    /// Clause 4.21: `id`, `type` and `scope` are structural keywords, every
    /// other stored name is an expanded attribute IRI.
    #[test]
    fn pick_and_omit_compile_to_keyword_and_attribute_terms() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": ["id", "https://example.com/speed"],
            "omit": ["scope", "type"]
        }));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        pretty_assertions::assert_eq!(
            projection.pick,
            vec![
                ProjectionTerm::Id,
                ProjectionTerm::Attribute {
                    iri: IriBuf::new("https://example.com/speed".to_owned()).expect("valid IRI"),
                    linked: Vec::new()
                }
            ]
        );
        pretty_assertions::assert_eq!(projection.omit, vec![ProjectionTerm::Scope, ProjectionTerm::Type]);
    }

    #[test]
    fn a_subscription_without_pick_or_omit_compiles_to_the_empty_projection() {
        let notification = notification(serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}}));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        assert!(projection.pick.is_empty() && projection.omit.is_empty());
    }

    #[test]
    fn a_name_that_is_not_an_expanded_iri_is_left_out_of_the_projection() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": ["speed", "https://example.com/colour"]
        }));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        pretty_assertions::assert_eq!(projection.pick.len(), 1);
    }

    fn names(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse().expect("valid attribute name")).collect()
    }

    /// Clauses 5.11.7 and 5.12: the attribute identifiers are the combination
    /// of both lists, and neither naming one means every attribute.
    #[test]
    fn csource_matching_attributes_is_the_union_of_both_lists_and_empty_when_neither_names_one() {
        let watched = names(&["speed"]);
        let notified = names(&["brandName", "speed"]);
        pretty_assertions::assert_eq!(csource_matching_attributes(Some(&watched), Some(&notified)), names(&["speed", "brandName"]));
        pretty_assertions::assert_eq!(csource_matching_attributes(Some(&watched), None), names(&["speed"]));
        pretty_assertions::assert_eq!(csource_matching_attributes(None, Some(&notified)), names(&["brandName", "speed"]));
        pretty_assertions::assert_eq!(csource_matching_attributes(Some(&watched), Some(&[])), names(&["speed"]));
        assert_eq!(csource_matching_attributes(None, None), [] as [AttributeName; 0]);
    }

    #[test]
    fn a_temporal_q_compiles_to_the_window_and_timeproperty_it_names() {
        let compiled = compile_temporal_q(
            &serde_json::json!({"timerel": "between", "timeAt": "2020-06-01T00:00:00Z", "endTimeAt": "2020-09-01T00:00:00Z", "timeproperty": "createdAt"}),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("temporalQ compiles");
        assert!(matches!(compiled.interval, TimeInterval::Between { .. }));
        pretty_assertions::assert_eq!(compiled.timeproperty, Some(TimeProp::Created));
        let after = compile_temporal_q(
            &serde_json::json!({"timerel": "after", "timeAt": "2020-06-01T00:00:00Z"}),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("temporalQ compiles");
        assert!(matches!(after.interval, TimeInterval::After(_)));
        pretty_assertions::assert_eq!(after.timeproperty, None);
    }

    #[test]
    fn a_malformed_temporal_q_does_not_compile() {
        assert!(compile_temporal_q(&serde_json::json!({"timerel": "after"}), "urn:ngsi-ld:Subscription:test").is_none());
        assert!(
            compile_temporal_q(
                &serde_json::json!({"timerel": "soon", "timeAt": "2020-06-01T00:00:00Z"}),
                "urn:ngsi-ld:Subscription:test"
            )
            .is_none()
        );
        assert!(
            compile_temporal_q(
                &serde_json::json!({"timerel": "between", "timeAt": "2020-06-01T00:00:00Z"}),
                "urn:ngsi-ld:Subscription:test"
            )
            .is_none()
        );
        assert!(compile_temporal_q(&serde_json::json!("after"), "urn:ngsi-ld:Subscription:test").is_none());
    }
}
