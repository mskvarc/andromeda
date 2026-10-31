//! Compilation of persisted subscriptions into matching state.

use crate::{
    compiled::{CompiledCsrSubscription, CompiledSubscription},
    geo_filter::SubscriptionGeoFilter,
    stored_subscription::StoredSubscription,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    notification::effective_notification_triggers,
    query::temporal_query::TemporalQuery,
    subscription::{NotificationParams, Subscription, entity_selector::EntitySelector},
};
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    filter::language_matching::LanguageMatching,
    projection::{
        compiled::Projection,
        grammar::{ProjectionList, RawProjectionTerm},
        term::ProjectionTerm,
    },
    temporal::window::TimeInterval,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use iri_rs::IriBuf;
use std::collections::HashSet;
use tracing::{debug, error};

/// Compile a stored subscription into a pre-compiled form for fast matching.
///
/// Takes the stored pair rather than the resource alone: matching ends in a
/// delivery enqueued against the surrogate key, so the key has to survive
/// compilation alongside everything the match itself needs. The filters'
/// target languages select keys by `language_matching`, the broker's live
/// mode.
pub fn compile_subscription(stored: &StoredSubscription, language_matching: LanguageMatching) -> CompiledSubscription {
    let sub = &stored.subscription;
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    let triggers = effective_notification_triggers(sub.selection.notification_trigger.as_deref());
    let watched_attrs = sub.selection.watched_attributes.clone().map(HashSet::from_iter);
    CompiledSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        key: stored.key,
        standing: sub.lifecycle.standing,
        endpoint_uri: sub.notification.endpoint.uri.url().clone(), // clone: compiled state owns the URL every delivery is built from.
        endpoint_accept: sub.notification.endpoint.accept,
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_cooldown: sub.notification.endpoint.cooldown,
        endpoint_timeout: sub.notification.endpoint.timeout,
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        notification_attributes: notification_attributes(&sub.notification, sub.id.as_ref()),
        notification_projection: notification_projection(&sub.notification, sub.id.as_ref()),
        notification_format: sub.notification.format,
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        notifying_since: stored.notifying_since,
        compiled_q: sub.filters.q.clone(), // clone: compiled state owns its filter AST.
        language_matching,
        compiled_geo_q: SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref()),
        compiled_scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        dataset_id: sub.filters.dataset_id.clone(),    // clone: compiled state owns dataset identifiers.
        notification_join: sub.notification.join,
        notification_join_level: sub.notification.effective_join_level(),
        compiled_csf: sub.filters.csf.clone(), // clone: compiled state owns its filter AST.
        local_only: sub.delivery.local_only.unwrap_or(false),
        ngsild_conformance: sub.delivery.ngsild_conformance,
        lang: sub.delivery.lang.clone(), // clone: compiled state owns the language tag.
    }
}

/// Compile a context-source subscription for in-memory registration matching
/// (clauses 5.11.2.4 and 5.12).
#[must_use]
pub fn compile_csource_subscription(sub: &Subscription) -> CompiledCsrSubscription {
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    CompiledCsrSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        standing: sub.lifecycle.standing,
        endpoint_uri: sub.notification.endpoint.uri.url().clone(), // clone: compiled state owns the URL every delivery is built from.
        endpoint_accept: sub.notification.endpoint.accept,
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_cooldown: sub.notification.endpoint.cooldown,
        endpoint_timeout: sub.notification.endpoint.timeout,
        selectors,
        matching_attrs: csource_matching_attributes(sub.selection.watched_attributes.as_deref(), sub.notification.attributes.as_deref()),
        temporal_q: sub
            .filters
            .temporal_q
            .as_ref()
            .and_then(|temporal_q| compile_temporal_q(temporal_q, sub.id.as_ref())),
        geo_q: SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref()),
        scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        compiled_csf: sub.filters.csf.clone(),               // clone: compiled state owns its filter AST.
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
/// `None` when the window the `temporalQ` names is not one, such as a
/// `between` that names no end. Admission refuses such a `temporalQ` with the
/// same rule (Table 5.2.21-1), so only a row written outside the broker reaches
/// this, and it is reported rather than matched as an unconstrained one silently.
pub fn compile_temporal_q(temporal_q: &TemporalQuery, sub_id: &str) -> Option<CompiledCsrTemporalFilter> {
    match TimeInterval::from_parts(Some(temporal_q.timerel), Some(temporal_q.time_at), temporal_q.end_time_at) {
        Ok(interval) => Some(
            CompiledCsrTemporalFilter::builder()
                .interval(interval)
                .timeproperty(temporal_q.timeproperty)
                .build(),
        ),
        Err(error) => {
            error!(subscription = sub_id, %error, "Stored subscription temporalQ names no window and is ignored");
            None
        }
    }
}

/// Compile the clause 5.2.14 `pick` and `omit` members into the projection a
/// query's `pick`/`omit` parameters compile to (clause 4.21).
///
/// The stored names were expanded through the subscription's `@context` when
/// the resource was written (clause 5.5.7), at every nesting level, except the
/// three clause 4.21 keywords, which name structural members of the
/// representation rather than attributes and are kept as written. The terms of
/// every string of one member form one list.
#[must_use]
pub fn notification_projection(notification: &NotificationParams, sub_id: &str) -> Projection {
    Projection::builder()
        .pick(projection_terms(notification.pick.as_deref().unwrap_or_default(), sub_id))
        .omit(projection_terms(notification.omit.as_deref().unwrap_or_default(), sub_id))
        .build()
}

/// Compile the clause 5.2.14 `attributes` member, "a synonym for pick"
/// (Table 5.2.14.1-1), into the expanded IRIs it lists.
///
/// The stored names were expanded through the subscription's `@context` when
/// the resource was written (clause 5.5.7). An absent member compiles to an
/// empty list, which narrows nothing.
#[must_use]
pub fn notification_attributes(notification: &NotificationParams, sub_id: &str) -> Vec<IriBuf> {
    notification
        .attributes
        .iter()
        .flatten()
        .filter_map(|name| expanded_attribute(name.as_str(), sub_id))
        .collect()
}

/// The compiled terms of one stored `pick` or `omit` member.
fn projection_terms(lists: &[ProjectionList], sub_id: &str) -> Vec<ProjectionTerm> {
    lists.iter().flat_map(|list| compiled_terms(list, sub_id)).collect()
}

/// The compiled form of stored terms, recursing into each attribute term's
/// clause 4.21 linked-entity sub-projection.
fn compiled_terms(terms: &[RawProjectionTerm], sub_id: &str) -> Vec<ProjectionTerm> {
    terms
        .iter()
        .filter_map(|term| match ProjectionTerm::keyword(term.name.as_str()) {
            Some(keyword) => Some(keyword),
            None => expanded_attribute(term.name.as_str(), sub_id).map(|iri| ProjectionTerm::Attribute {
                iri,
                linked: compiled_terms(&term.linked, sub_id),
            }),
        })
        .collect()
}

/// The expanded IRI a stored attribute name spells.
///
/// A name that is not an absolute IRI can address nothing in an expanded
/// entity, so it is left out rather than carried as a name that never matches.
fn expanded_attribute(name: &str, sub_id: &str) -> Option<IriBuf> {
    match IriBuf::new(name.to_owned()) {
        Ok(iri) => Some(iri),
        Err(error) => {
            debug!(subscription = sub_id, name, %error, "Subscription projection member is not an expanded attribute name");
            None
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::compilation::{compile_temporal_q, csource_matching_attributes, notification_attributes, notification_projection};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{query::temporal_query::TemporalQuery, subscription::NotificationParams};
    use andromeda_ngsi_ld_query::{
        projection::{grammar::ProjectionList, term::ProjectionTerm},
        temporal::window::TimeInterval,
    };
    use iri_rs::IriBuf;

    fn notification(value: serde_json::Value) -> NotificationParams {
        serde_json::from_value(value).expect("valid notification parameters")
    }

    /// The stored tree of one clause 4.21 string per element.
    fn stored(text: &[&str]) -> serde_json::Value {
        let lists: Vec<ProjectionList> = text.iter().map(|raw| raw.parse().expect("valid projection list")).collect();
        serde_json::to_value(lists).expect("serializes")
    }

    fn attribute(iri: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new(iri.to_owned()).expect("valid IRI"),
            linked,
        }
    }

    /// Clause 4.21: `id`, `type` and `scope` are structural keywords, every
    /// other stored name is an expanded attribute IRI.
    #[test]
    fn pick_and_omit_compile_to_keyword_and_attribute_terms() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": stored(&["id", "https://example.com/speed"]),
            "omit": stored(&["scope,type"])
        }));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        pretty_assertions::assert_eq!(projection.pick, vec![ProjectionTerm::Id, attribute("https://example.com/speed", Vec::new())]);
        pretty_assertions::assert_eq!(projection.omit, vec![ProjectionTerm::Scope, ProjectionTerm::Type]);
    }

    /// Clause 4.21: a nested group compiles to the linked sub-projection of
    /// its Relationship term, at every level.
    #[test]
    fn a_nested_pick_compiles_to_a_linked_term() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": stored(&["https://example.com/owner{id,https://example.com/employer{https://example.com/size}}"])
        }));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        pretty_assertions::assert_eq!(
            projection.pick,
            vec![attribute(
                "https://example.com/owner",
                vec![
                    ProjectionTerm::Id,
                    attribute("https://example.com/employer", vec![attribute("https://example.com/size", Vec::new())])
                ]
            )]
        );
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
            "pick": stored(&["speed", "https://example.com/colour"])
        }));

        let projection = notification_projection(&notification, "urn:ngsi-ld:Subscription:test");

        pretty_assertions::assert_eq!(projection.pick.len(), 1);
    }

    /// Table 5.2.14.1-1: `attributes` lists expanded attribute names, and a
    /// name that is no IRI addresses nothing.
    #[test]
    fn notification_attributes_compile_as_the_pick_synonym() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "attributes": ["https://example.com/speed", "colour"]
        }));

        pretty_assertions::assert_eq!(
            notification_attributes(&notification, "urn:ngsi-ld:Subscription:test"),
            vec![IriBuf::new("https://example.com/speed".to_owned()).expect("valid IRI")]
        );
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

    /// The `temporalQ` a request writes, read as the request model reads it.
    fn temporal_q(wire: serde_json::Value) -> TemporalQuery {
        serde_json::from_value(wire).expect("a readable temporalQ")
    }

    #[test]
    fn a_temporal_q_compiles_to_the_window_and_timeproperty_it_names() {
        let compiled = compile_temporal_q(
            &temporal_q(
                serde_json::json!({"timerel": "between", "timeAt": "2020-06-01T00:00:00Z", "endTimeAt": "2020-09-01T00:00:00Z", "timeproperty": "createdAt"}),
            ),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("temporalQ compiles");
        assert!(matches!(compiled.interval, TimeInterval::Between { .. }));
        pretty_assertions::assert_eq!(compiled.timeproperty, Some(TimeProp::Created));
        let after = compile_temporal_q(
            &temporal_q(serde_json::json!({"timerel": "after", "timeAt": "2020-06-01T00:00:00Z"})),
            "urn:ngsi-ld:Subscription:test",
        )
        .expect("temporalQ compiles");
        assert!(matches!(after.interval, TimeInterval::After(_)));
        pretty_assertions::assert_eq!(after.timeproperty, None);
    }

    /// A `between` that names no end is no window, so the subscription is
    /// matched as one without a temporal query. A `temporalQ` that names no
    /// `timeAt`, or a relation outside clause 4.11, is no `temporalQ` at all and
    /// is refused when the body is read.
    #[test]
    fn a_between_without_an_end_does_not_compile() {
        assert!(
            compile_temporal_q(
                &temporal_q(serde_json::json!({"timerel": "between", "timeAt": "2020-06-01T00:00:00Z"})),
                "urn:ngsi-ld:Subscription:test"
            )
            .is_none()
        );
        assert!(serde_json::from_value::<TemporalQuery>(serde_json::json!({"timerel": "after"})).is_err());
        assert!(serde_json::from_value::<TemporalQuery>(serde_json::json!({"timerel": "soon", "timeAt": "2020-06-01T00:00:00Z"})).is_err());
    }
}
