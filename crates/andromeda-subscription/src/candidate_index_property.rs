//! Property tests pinning the candidate index to the authoritative matcher.
//!
//! The index is only allowed to be a superset filter, so over random
//! subscriptions and events its matches must equal a full scan with
//! [`matches_subscription`], its funnel must shrink monotonically, and an
//! index that had entries removed must behave like one rebuilt from scratch.

use crate::{candidate_index::CandidateIndex, compiled::CompiledSubscription, error::SubscriptionError, matching::matches_subscription};
use andromeda_context_commit::{change_event::ChangeEvent, subscription_key::SubscriptionKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    notification::{ChangeOrigin, NotificationTrigger},
    subscription::{EntitySelector, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use proptest::{
    bool::ANY,
    collection::vec,
    option,
    prelude::{Just, Strategy, prop_assert, prop_assert_eq, prop_oneof, proptest},
    sample::{select, subsequence},
    test_runner::{Config, TestCaseError},
};
use std::{
    collections::{BTreeSet, HashSet},
    num::NonZeroU32,
    sync::Arc,
};
use strum::IntoEnumIterator;
use url::Url;

const TYPES: [&str; 3] = ["Building", "Vehicle", "Sensor"];
const ATTRIBUTES: [&str; 3] = ["temperature", "speed", "name"];
const ID_PATTERNS: [&str; 2] = ["^urn:ngsi-ld:E:1$", "urn:ngsi-ld:E:.*"];

#[derive(Debug, Clone)]
enum TypeDesc {
    Wildcard,
    Single(usize),
    Or(usize, usize),
    And(usize, usize),
}

#[derive(Debug, Clone)]
struct SelectorDesc {
    type_expr: TypeDesc,
    id: Option<usize>,
    id_pattern: Option<usize>,
}

#[derive(Debug, Clone)]
struct SubscriptionDesc {
    selectors: Vec<SelectorDesc>,
    triggers: Vec<NotificationTrigger>,
    watched: Option<Vec<usize>>,
}

#[derive(Debug, Clone)]
struct EventDesc {
    types: Vec<usize>,
    id: usize,
    trigger: NotificationTrigger,
    attributes: Vec<usize>,
}

fn type_desc() -> impl Strategy<Value = TypeDesc> {
    prop_oneof![
        Just(TypeDesc::Wildcard),
        (0..TYPES.len()).prop_map(TypeDesc::Single),
        (0..TYPES.len(), 0..TYPES.len()).prop_map(|(left, right)| TypeDesc::Or(left, right)),
        (0..TYPES.len(), 0..TYPES.len()).prop_map(|(left, right)| TypeDesc::And(left, right)),
    ]
}

fn selector_desc() -> impl Strategy<Value = SelectorDesc> {
    (type_desc(), option::of(0..4_usize), option::of(0..ID_PATTERNS.len())).prop_map(|(type_expr, id, id_pattern)| SelectorDesc { type_expr, id, id_pattern })
}

fn subscription_desc() -> impl Strategy<Value = SubscriptionDesc> {
    let triggers: Vec<NotificationTrigger> = NotificationTrigger::iter().collect();
    (
        vec(selector_desc(), 0..3),
        subsequence(triggers, 0..=NotificationTrigger::iter().count()),
        option::of(subsequence((0..ATTRIBUTES.len()).collect::<Vec<_>>(), 0..=ATTRIBUTES.len())),
    )
        .prop_map(|(selectors, triggers, watched)| SubscriptionDesc { selectors, triggers, watched })
}

fn event_desc() -> impl Strategy<Value = EventDesc> {
    let triggers: Vec<NotificationTrigger> = NotificationTrigger::iter().collect();
    (
        subsequence((0..TYPES.len()).collect::<Vec<_>>(), 0..=TYPES.len()),
        0..4_usize,
        select(triggers),
        subsequence((0..ATTRIBUTES.len()).collect::<Vec<_>>(), 0..=ATTRIBUTES.len()),
    )
        .prop_map(|(types, id, trigger, attributes)| EventDesc {
            types,
            id,
            trigger,
            attributes,
        })
}

fn internal(error: &impl ToString) -> SubscriptionError {
    SubscriptionError::internal(error.to_string())
}

fn entity_uri(index: usize) -> Result<Uri, SubscriptionError> {
    format!("urn:ngsi-ld:E:{index}").parse().map_err(|error| internal(&error))
}

fn type_expression(desc: &TypeDesc) -> String {
    match desc {
        TypeDesc::Wildcard => "*".to_owned(),
        TypeDesc::Single(index) => TYPES[*index].to_owned(),
        TypeDesc::Or(left, right) => format!("({}|{})", TYPES[*left], TYPES[*right]),
        TypeDesc::And(left, right) => format!("({};{})", TYPES[*left], TYPES[*right]),
    }
}

fn build_subscription(ordinal: usize, desc: &SubscriptionDesc) -> Result<CompiledSubscription, SubscriptionError> {
    let selectors = desc
        .selectors
        .iter()
        .map(|selector| {
            Ok(EntitySelector {
                id: selector.id.map(entity_uri).transpose()?,
                id_pattern: selector
                    .id_pattern
                    .map(|index| ID_PATTERNS[index].parse().map_err(|error| internal(&error)))
                    .transpose()?,
                type_: type_expression(&selector.type_expr).parse().map_err(|error| internal(&error))?,
            })
        })
        .collect::<Result<Vec<_>, SubscriptionError>>()?;
    let watched_attrs = desc
        .watched
        .as_ref()
        .map(|watched| {
            watched
                .iter()
                .map(|index| ATTRIBUTES[*index].parse::<AttributeName>().map_err(|error| internal(&error)))
                .collect::<Result<HashSet<_>, SubscriptionError>>()
        })
        .transpose()?;
    Ok(CompiledSubscription {
        id: format!("urn:ngsi-ld:Subscription:{ordinal}").parse().map_err(|error| internal(&error))?,
        key: SubscriptionKey::try_from(i64::try_from(ordinal + 1).map_err(|error| internal(&error))?).map_err(|error| internal(&error))?,
        status: SubscriptionStatus::Active,
        endpoint_uri: Url::parse("http://example.com/notify").map_err(|error| internal(&error))?,
        endpoint_accept: None,
        endpoint_receiver_info: None,
        endpoint_notifier_info: None,
        endpoint_cooldown: None,
        endpoint_timeout: None,
        triggers: desc.triggers.clone(),
        watched_attrs,
        selectors,
        throttling_secs: None,
        time_interval_secs: None,
        notification_attributes: None,
        notification_format: None,
        notification_sys_attrs: SysAttrsInclusion::Strip,
        notification_show_changes: false,
        jsonld_context: None,
        expires_at: None,
        created_at: DateTime::<Utc>::MIN_UTC,
        compiled_q: None,
        compiled_geo_q: None,
        compiled_scope_q: None,
        dataset_id: None,
        notification_join: None,
        notification_join_level: NonZeroU32::MIN,
        compiled_csf: None,
        local_only: false,
        ngsild_conformance: None,
        notification_projection: Projection::default(),
    })
}

fn build_event(desc: &EventDesc) -> Result<ChangeEvent, SubscriptionError> {
    Ok(ChangeEvent::builder()
        .entity_id(entity_uri(desc.id)?)
        .entity_types(
            desc.types
                .iter()
                .map(|index| TYPES[*index].parse::<EntityTypeName>().map_err(|error| internal(&error)))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .trigger_reason(desc.trigger)
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
        .changed_attributes(
            desc.attributes
                .iter()
                .map(|index| ATTRIBUTES[*index].parse::<AttributeName>().map_err(|error| internal(&error)))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .build())
}

fn matched_ids(subscriptions: &[Arc<CompiledSubscription>]) -> BTreeSet<String> {
    subscriptions.iter().map(|subscription| subscription.id.to_string()).collect()
}

fn full_scan(subscriptions: &[Arc<CompiledSubscription>], event: &ChangeEvent) -> BTreeSet<String> {
    subscriptions
        .iter()
        .filter(|subscription| matches_subscription(subscription, event))
        .map(|subscription| subscription.id.to_string())
        .collect()
}

fn fail(error: &SubscriptionError) -> TestCaseError {
    TestCaseError::fail(error.to_string())
}

proptest! {
    #![proptest_config(Config::with_cases(2000))]

    #[test]
    fn indexed_candidates_match_exactly_what_a_full_scan_matches(
        subscriptions in vec(subscription_desc(), 1..12),
        events in vec(event_desc(), 1..6),
    ) {
        let compiled: Vec<Arc<CompiledSubscription>> = subscriptions
            .iter()
            .enumerate()
            .map(|(ordinal, desc)| build_subscription(ordinal, desc).map(Arc::new))
            .collect::<Result<_, _>>()
            .map_err(|error| fail(&error))?;
        let mut index = CandidateIndex::default();
        for subscription in &compiled {
            index.insert(Arc::clone(subscription));
        }
        for desc in &events {
            let event = build_event(desc).map_err(|error| fail(&error))?;
            let outcome = index.find_matching(&event);
            prop_assert_eq!(matched_ids(&outcome.matched), full_scan(&compiled, &event));
            let funnel = outcome.funnel;
            prop_assert!(
                funnel.active >= funnel.routed && funnel.routed >= funnel.watched && funnel.watched >= funnel.evaluated && funnel.evaluated >= funnel.matched,
                "funnel is not monotone: {funnel:?}"
            );
            prop_assert_eq!(funnel.active, compiled.len());
        }
    }

    #[test]
    fn removing_entries_leaves_the_index_equal_to_one_rebuilt_from_the_survivors(
        subscriptions in vec(subscription_desc(), 1..12),
        removed in vec(ANY, 12),
        events in vec(event_desc(), 1..6),
    ) {
        let compiled: Vec<Arc<CompiledSubscription>> = subscriptions
            .iter()
            .enumerate()
            .map(|(ordinal, desc)| build_subscription(ordinal, desc).map(Arc::new))
            .collect::<Result<_, _>>()
            .map_err(|error| fail(&error))?;
        let mut pruned = CandidateIndex::default();
        let mut rebuilt = CandidateIndex::default();
        for subscription in &compiled {
            pruned.insert(Arc::clone(subscription));
        }
        for (subscription, remove) in compiled.iter().zip(&removed) {
            if *remove {
                pruned.remove(&subscription.id);
            } else {
                rebuilt.insert(Arc::clone(subscription));
            }
        }
        prop_assert_eq!(pruned.len(), rebuilt.len());
        for desc in &events {
            let event = build_event(desc).map_err(|error| fail(&error))?;
            let from_pruned = pruned.find_matching(&event);
            let from_rebuilt = rebuilt.find_matching(&event);
            prop_assert_eq!(matched_ids(&from_pruned.matched), matched_ids(&from_rebuilt.matched));
            prop_assert_eq!(from_pruned.funnel, from_rebuilt.funnel);
        }
    }
}
