//! Compilation of persisted subscriptions into matching state.

use crate::{
    compiled::{CompiledCsrSubscription, CompiledSubscription},
    geo_filter::SubscriptionGeoFilter,
    stored_subscription::StoredSubscription,
    subscription_query::SubscriptionQuery,
};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_model::{
    entity_selector::EntitySelector,
    notification::effective_notification_triggers,
    query::temporal_query::TemporalQuery,
    subscription::{NotificationParams, Subscription},
};
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching, reading::filter_reading::FilterReading},
    name_stage::Expanded,
    projection::{expanded_projection::ExpandedProjection, expanded_projection_list::ExpandedProjectionList, expanded_projection_term::ExpandedProjectionTerm},
    temporal::window::TimeInterval,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use iri_rs::IriBuf;
use tracing::error;

/// Compile a stored subscription into a pre-compiled form for fast matching.
///
/// Takes the stored pair rather than the resource alone: matching ends in a
/// delivery enqueued against the surrogate key, so the key has to survive
/// compilation alongside everything the match itself needs. The filters'
/// target languages select keys by `language_matching`, the broker's live
/// mode.
#[must_use]
pub fn compile_subscription(stored: &StoredSubscription, language_matching: LanguageMatching) -> CompiledSubscription {
    let sub = &stored.subscription;
    let selectors = stored_selectors(sub);
    let triggers = effective_notification_triggers(sub.selection.notification_trigger.as_deref());
    // clone: compiled state owns its names apart from the persisted value
    let watched_attrs = sub.selection.watched_attributes.as_deref().map(|names| names.iter().cloned().collect());
    let compiled_q = if sub.unreadable.is_empty() {
        // clone: compiled state owns its filter AST.
        sub.filters.q.clone().map(|q| SubscriptionQuery::new(CompiledFilter::new(q, language_matching)))
    } else {
        Some(fail_closed_query(language_matching))
    };
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
        notification_attributes: notification_attributes(&sub.notification),
        notification_projection: notification_projection(&sub.notification),
        notification_format: sub.notification.format,
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        notifying_since: stored.notifying_since,
        compiled_q,
        compiled_geo_q: stored_geo_filter(sub),
        compiled_scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        dataset_id: sub.filters.dataset_id.clone(),    // clone: compiled state owns dataset identifiers.
        notification_join: sub.notification.join,
        notification_join_level: sub.notification.effective_join_level(),
        local_only: sub.delivery.local_only.unwrap_or(false),
        ngsild_conformance: sub.delivery.ngsild_conformance,
        lang: sub.delivery.lang.clone(), // clone: compiled state owns the language filter.
    }
}

/// Compile a context-source subscription for in-memory registration matching
/// (clauses 5.11.2.4 and 5.12).
#[must_use]
pub fn compile_csource_subscription(sub: &Subscription) -> CompiledCsrSubscription {
    let selectors = stored_selectors(sub);
    let compiled_csf = if sub.unreadable.is_empty() {
        sub.filters.csf.as_ref().map(FilterReading::of_context_source_filter)
    } else {
        Some(FilterReading::of_context_source_filter(&Filter::Or(Vec::new())))
    };
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
        geo_q: stored_geo_filter(sub),
        scope_q: sub.filters.scope_q.clone(),       // clone: compiled state owns its scope AST.
        dataset_id: sub.filters.dataset_id.clone(), // clone: compiled state owns dataset identifiers.
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        compiled_csf,
    }
}

/// The stored `entities` of `sub` as compiled state holds them: none when the
/// store listed a member it could not read, since the subscription then selects
/// nothing.
fn stored_selectors(sub: &Subscription) -> Vec<EntitySelector<Expanded>> {
    if !sub.unreadable.is_empty() {
        let members = sub.unreadable.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        error!(subscription = %sub.id, members, "a stored member is not in the expanded stage, so the subscription selects nothing");
        return Vec::new();
    }
    // clone: compiled state owns its selectors apart from the persisted value
    sub.selection.entities.clone().unwrap_or_default()
}

/// The query no entity satisfies, for a subscription whose stored members cannot be read.
fn fail_closed_query(language_matching: LanguageMatching) -> SubscriptionQuery {
    SubscriptionQuery::new(CompiledFilter::new(Filter::<Expanded>::Or(Vec::new()), language_matching))
}

/// The geo restriction of a stored subscription: none that selects an entity
/// when a stored member could not be read.
fn stored_geo_filter(sub: &Subscription) -> SubscriptionGeoFilter {
    if sub.unreadable.is_empty() {
        SubscriptionGeoFilter::from_stored(sub.filters.geo_q.as_ref(), sub.id.as_ref())
    } else {
        SubscriptionGeoFilter::Unusable
    }
}

/// The attribute identifiers a context-source subscription matches with
/// (clauses 5.11.7 and 5.12): the combination of `watchedAttributes` and
/// `notification.attributes`, without repetition. Empty, when neither names an
/// attribute, stands for every attribute.
fn csource_matching_attributes(watched: Option<&[AttributeIri]>, notified: Option<&[AttributeIri]>) -> Vec<AttributeIri> {
    let mut combined: Vec<AttributeIri> = Vec::new();
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

/// Compile the clause 4.21 `pick` and `omit` members of a stored notification.
///
/// The stored names were expanded through the subscription's `@context` when
/// the resource was written (clause 5.5.7), at every nesting level, except the
/// three clause 4.21 keywords, which name structural members of the
/// representation rather than attributes. The terms of every string of one
/// member form one list.
#[must_use]
pub fn notification_projection(notification: &NotificationParams) -> ExpandedProjection {
    let terms = |lists: &Option<Vec<ExpandedProjectionList>>| -> Vec<ExpandedProjectionTerm> {
        // clone: compiled state owns its projection terms apart from the persisted value
        lists.iter().flatten().flat_map(|list| list.iter().cloned()).collect()
    };
    ExpandedProjection::builder()
        .pick(terms(&notification.pick))
        .omit(terms(&notification.omit))
        .build()
}

/// Compile the clause 5.2.14 `attributes` member, "a synonym for pick"
/// (Table 5.2.14.1-1), into the expanded IRIs it lists. An absent member
/// compiles to an empty list, which narrows nothing.
#[must_use]
pub fn notification_attributes(notification: &NotificationParams) -> Vec<IriBuf> {
    notification.attributes.iter().flatten().map(|name| name.as_iri().clone()).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        compilation::{
            compile_csource_subscription,
            compile_subscription,
            compile_temporal_q,
            csource_matching_attributes,
            notification_attributes,
            notification_projection,
        },
        geo_filter::SubscriptionGeoFilter,
        notifying_since::NotifyingSince,
        stored_subscription::StoredSubscription,
    };
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::{
        query::temporal_query::TemporalQuery,
        subscription::{NotificationParams, Subscription, unreadable_member::UnreadableMember},
    };
    use andromeda_ngsi_ld_query::{
        filter::language_matching::LanguageMatching,
        projection::{expanded_projection_term::ExpandedProjectionTerm, written_projection_list::WrittenProjectionList},
        temporal::window::TimeInterval,
    };
    use chrono::DateTime;
    use iri_rs::IriBuf;
    use strum::IntoEnumIterator;

    fn notification(value: serde_json::Value) -> NotificationParams {
        serde_json::from_value(value).expect("valid notification parameters")
    }

    /// The stored tree of one clause 4.21 string per element.
    fn stored(text: &[&str]) -> serde_json::Value {
        let lists: Vec<WrittenProjectionList> = text.iter().map(|raw| raw.parse().expect("valid projection list")).collect();
        serde_json::to_value(lists).expect("serializes")
    }

    fn attribute(iri: &str, linked: Vec<ExpandedProjectionTerm>) -> ExpandedProjectionTerm {
        ExpandedProjectionTerm::Attribute {
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

        let projection = notification_projection(&notification);

        pretty_assertions::assert_eq!(
            projection.pick,
            vec![ExpandedProjectionTerm::Id, attribute("https://example.com/speed", Vec::new())]
        );
        pretty_assertions::assert_eq!(projection.omit, vec![ExpandedProjectionTerm::Scope, ExpandedProjectionTerm::Type]);
    }

    /// Clause 4.21: a nested group compiles to the linked sub-projection of
    /// its Relationship term, at every level.
    #[test]
    fn a_nested_pick_compiles_to_a_linked_term() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": stored(&["https://example.com/owner{id,https://example.com/employer{https://example.com/size}}"])
        }));

        let projection = notification_projection(&notification);

        pretty_assertions::assert_eq!(
            projection.pick,
            vec![attribute(
                "https://example.com/owner",
                vec![
                    ExpandedProjectionTerm::Id,
                    attribute("https://example.com/employer", vec![attribute("https://example.com/size", Vec::new())])
                ]
            )]
        );
    }

    #[test]
    fn a_subscription_without_pick_or_omit_compiles_to_the_empty_projection() {
        let notification = notification(serde_json::json!({"endpoint": {"uri": "https://receiver.example.com/notify"}}));

        let projection = notification_projection(&notification);

        assert!(projection.pick.is_empty() && projection.omit.is_empty());
    }

    #[test]
    fn a_projection_list_holding_a_name_that_is_no_expanded_iri_is_read_as_absent_and_no_list_is_kept() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": stored(&["speed", "https://example.com/colour"])
        }));

        assert!(notification.pick.is_none(), "{notification:?}");
        pretty_assertions::assert_eq!(notification_projection(&notification).pick.len(), 0);
    }

    /// Table 5.2.14.1-1: `attributes` lists expanded attribute names.
    #[test]
    fn notification_attributes_compile_as_the_pick_synonym() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "attributes": ["https://example.com/speed", "https://example.com/colour"]
        }));

        pretty_assertions::assert_eq!(
            notification_attributes(&notification),
            vec![
                IriBuf::new("https://example.com/speed".to_owned()).expect("valid IRI"),
                IriBuf::new("https://example.com/colour".to_owned()).expect("valid IRI")
            ]
        );
    }

    fn names(raw: &[&str]) -> Vec<AttributeIri> {
        raw.iter()
            .map(|name| format!("https://example.com/{name}").parse().expect("valid attribute IRI"))
            .collect()
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
        assert_eq!(csource_matching_attributes(None, None), [] as [AttributeIri; 0]);
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

    /// A subscription the store listed a member of as unreadable.
    fn subscription_with_unreadable_member() -> Subscription {
        let mut subscription: Subscription = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Subscription:corrupt",
            "type": "Subscription",
            "entities": [{"type": "https://example.com/Vehicle"}],
            "notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("a valid subscription");
        subscription.unreadable = vec![UnreadableMember::Q];
        subscription
    }

    #[test]
    fn an_entity_subscription_with_an_unreadable_member_selects_no_entity() {
        let at = StoreInstant::new(DateTime::from_timestamp(0, 0).expect("an instant in range"));
        let stored = StoredSubscription {
            key: SubscriptionKey::try_from(9).expect("a surrogate key"),
            subscription: subscription_with_unreadable_member(),
            notifying_since: NotifyingSince { active: at, selected: at },
        };

        let compiled = compile_subscription(&stored, LanguageMatching::default());

        assert!(compiled.compiled_q.is_some(), "the filter no entity satisfies stands in");
        assert!(
            matches!(compiled.compiled_geo_q, SubscriptionGeoFilter::Unusable),
            "{:?}",
            compiled.compiled_geo_q
        );
        pretty_assertions::assert_eq!(compiled.selectors.len(), 0);
    }

    #[test]
    fn a_context_source_subscription_with_an_unreadable_member_selects_no_registration() {
        let compiled = compile_csource_subscription(&subscription_with_unreadable_member());

        assert!(compiled.compiled_csf.is_some(), "the filter no registration satisfies stands in");
        assert!(matches!(compiled.geo_q, SubscriptionGeoFilter::Unusable), "{:?}", compiled.geo_q);
    }

    #[test]
    fn a_notification_attribute_list_holding_a_name_that_is_no_absolute_iri_is_read_as_absent_for_the_store_to_list() {
        let notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "attributes": ["https://example.com/speed", "colour"]
        }));
        assert!(notification.attributes.is_none(), "{notification:?}");
        pretty_assertions::assert_eq!(notification_attributes(&notification).len(), 0);
    }

    #[test]
    fn a_subscription_whose_notification_attributes_are_unreadable_selects_nothing_whatever_it_listed() {
        let mut subscription = subscription_with_unreadable_member();
        subscription.notification = notification(serde_json::json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "attributes": ["bare"]
        }));
        subscription.unreadable = vec![UnreadableMember::NotificationAttributes];
        let at = StoreInstant::new(DateTime::from_timestamp(0, 0).expect("an instant in range"));
        let stored = StoredSubscription {
            key: SubscriptionKey::try_from(9).expect("a surrogate key"),
            subscription,
            notifying_since: NotifyingSince { active: at, selected: at },
        };

        let compiled = compile_subscription(&stored, LanguageMatching::default());

        pretty_assertions::assert_eq!(compiled.selectors.len(), 0);
        assert!(compiled.compiled_q.is_some(), "the filter no entity satisfies stands in");
        assert!(
            matches!(compiled.compiled_geo_q, SubscriptionGeoFilter::Unusable),
            "{:?}",
            compiled.compiled_geo_q
        );
    }

    #[test]
    fn every_unreadable_member_makes_an_entity_and_a_context_source_subscription_select_nothing() {
        let at = StoreInstant::new(DateTime::from_timestamp(0, 0).expect("an instant in range"));
        for member in UnreadableMember::iter() {
            let mut subscription = subscription_with_unreadable_member();
            subscription.unreadable = vec![member];

            let stored = StoredSubscription {
                key: SubscriptionKey::try_from(9).expect("a surrogate key"),
                subscription: subscription.clone(),
                notifying_since: NotifyingSince { active: at, selected: at },
            };
            let entity = compile_subscription(&stored, LanguageMatching::default());
            pretty_assertions::assert_eq!(entity.selectors.len(), 0, "{member}");
            assert!(matches!(entity.compiled_geo_q, SubscriptionGeoFilter::Unusable), "{member}");

            let csource = compile_csource_subscription(&subscription);
            pretty_assertions::assert_eq!(csource.selectors.len(), 0, "{member}");
            assert!(matches!(csource.geo_q, SubscriptionGeoFilter::Unusable), "{member}");
        }
    }
}
