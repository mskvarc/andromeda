//! Property tests pinning the candidate index to the authoritative matcher.
//!
//! The index is only allowed to be a superset filter, so over random
//! subscriptions and events its matches must equal a full scan with
//! [`matches_subscription`], its funnel must shrink monotonically, and an
//! index that had entries removed must behave like one rebuilt from scratch.
//! Beside them, examples pin how many Subscriptions on a type expression one
//! change routes.

use crate::{
    candidate_index::CandidateIndex,
    compiled::CompiledSubscription,
    error::SubscriptionError,
    geo_filter::SubscriptionGeoFilter,
    matching::matches_subscription,
    notifying_since::NotifyingSince,
};
use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_ngsi_ld_entity::reads::{
    attribute_change::{AttributeChange, AttributeChangeKind},
    attribute_instance_key::AttributeInstanceKey,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    entity_selector::EntitySelector,
    notification::{ChangeOrigin, NotificationTrigger},
    subscription::{SubscriptionStatus, standing::SubscriptionStanding},
};
use andromeda_ngsi_ld_query::projection::expanded_projection::ExpandedProjection;
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

const TYPES: [&str; 3] = ["https://example.com/Building", "https://example.com/Vehicle", "https://example.com/Sensor"];
const ATTRIBUTES: [&str; 3] = ["https://example.com/temperature", "https://example.com/speed", "https://example.com/name"];
const ID_PATTERNS: [&str; 2] = ["^urn:ngsi-ld:E:1$", "urn:ngsi-ld:E:.*"];

/// The position of the Building type in [`TYPES`].
const BUILDING: usize = 0;
/// The position of the Vehicle type in [`TYPES`].
const VEHICLE: usize = 1;
/// The position of the Sensor type in [`TYPES`].
const SENSOR: usize = 2;

#[derive(Debug, Clone)]
enum TypeDesc {
    Wildcard,
    /// The disjunction of these conjunctions of positions in [`TYPES`].
    Alternatives(Vec<Vec<usize>>),
}

#[derive(Debug, Clone)]
struct SelectorDesc {
    type_selection: TypeDesc,
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
    lifecycle: EntityLifecycle,
    instances: Vec<(usize, AttributeChangeKind)>,
}

/// The wildcard, or one to three alternatives of one or two types each: a
/// single type, a disjunction, a conjunction and every mix of them clause 4.17
/// writes.
fn type_desc() -> impl Strategy<Value = TypeDesc> {
    prop_oneof![
        1 => Just(TypeDesc::Wildcard),
        4 => vec(vec(0..TYPES.len(), 1..=2), 1..=3).prop_map(TypeDesc::Alternatives),
    ]
}

fn selector_desc() -> impl Strategy<Value = SelectorDesc> {
    (type_desc(), option::of(0..4_usize), option::of(0..ID_PATTERNS.len())).prop_map(|(type_selection, id, id_pattern)| SelectorDesc {
        type_selection,
        id,
        id_pattern,
    })
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
    let lifecycles = vec![EntityLifecycle::Created, EntityLifecycle::Modified, EntityLifecycle::Deleted];
    let kinds = vec![AttributeChangeKind::Created, AttributeChangeKind::Updated, AttributeChangeKind::Deleted];
    (
        subsequence((0..TYPES.len()).collect::<Vec<_>>(), 0..=TYPES.len()),
        0..4_usize,
        select(lifecycles),
        vec((0..ATTRIBUTES.len(), select(kinds)), 0..=ATTRIBUTES.len()),
    )
        .prop_map(|(types, id, lifecycle, instances)| EventDesc {
            types,
            id,
            lifecycle,
            instances,
        })
}

fn internal(error: &impl ToString) -> SubscriptionError {
    panic!("invalid test fixture: {}", error.to_string())
}

fn entity_uri(index: usize) -> Result<Identifier, SubscriptionError> {
    format!("urn:ngsi-ld:E:{index}").parse().map_err(|error| internal(&error))
}

fn type_selection_text(desc: &TypeDesc) -> String {
    match desc {
        TypeDesc::Wildcard => "*".to_owned(),
        TypeDesc::Alternatives(alternatives) => alternatives
            .iter()
            .map(|conjunction| match conjunction.as_slice() {
                [only] => TYPES[*only].to_owned(),
                several => format!("({})", several.iter().map(|index| TYPES[*index]).collect::<Vec<_>>().join(";")),
            })
            .collect::<Vec<_>>()
            .join("|"),
    }
}

fn build_subscription(ordinal: usize, desc: &SubscriptionDesc) -> Result<CompiledSubscription, SubscriptionError> {
    let selectors = desc
        .selectors
        .iter()
        .map(|selector| {
            Ok(EntitySelector {
                id: selector.id.map(entity_uri).transpose()?.map(|id| vec![id]),
                id_pattern: selector
                    .id_pattern
                    .map(|index| ID_PATTERNS[index].parse().map_err(|error| internal(&error)))
                    .transpose()?,
                type_: type_selection_text(&selector.type_selection).parse().map_err(|error| internal(&error))?,
            })
        })
        .collect::<Result<Vec<_>, SubscriptionError>>()?;
    let watched_attrs = desc
        .watched
        .as_ref()
        .map(|watched| {
            watched
                .iter()
                .map(|index| ATTRIBUTES[*index].parse::<AttributeIri>().map_err(|error| internal(&error)))
                .collect::<Result<HashSet<_>, SubscriptionError>>()
        })
        .transpose()?;
    Ok(CompiledSubscription {
        id: format!("urn:ngsi-ld:Subscription:{ordinal}").parse().map_err(|error| internal(&error))?,
        key: SubscriptionKey::try_from(i64::try_from(ordinal + 1).map_err(|error| internal(&error))?).map_err(|error| internal(&error))?,
        standing: SubscriptionStanding {
            status: SubscriptionStatus::Active,
            expires_at: None,
        },
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
        notification_attributes: Vec::new(),
        notification_format: None,
        notification_sys_attrs: SysAttrsInclusion::Strip,
        notification_show_changes: false,
        jsonld_context: None,
        notifying_since: NotifyingSince {
            active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
        },
        compiled_q: None,
        compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
        compiled_scope_q: None,
        dataset_id: None,
        notification_join: None,
        notification_join_level: NonZeroU32::MIN,
        local_only: false,
        ngsild_conformance: None,
        lang: None,
        notification_projection: ExpandedProjection::default(),
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
        .change(EntityChange::new(
            desc.lifecycle,
            desc.instances
                .iter()
                .map(|(index, kind)| {
                    Ok(AttributeChange {
                        key: AttributeInstanceKey {
                            attr: ATTRIBUTES[*index].parse::<AttributeName>().map_err(|error| internal(&error))?,
                            dataset_id: None,
                        },
                        kind: *kind,
                    })
                })
                .collect::<Result<Vec<_>, SubscriptionError>>()?,
        ))
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
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

/// An index of `count` Subscriptions, each with the one selector
/// `type_selection` and notified of creations.
fn index_on(type_selection: &TypeDesc, count: usize) -> Result<CandidateIndex, SubscriptionError> {
    let mut index = CandidateIndex::default();
    for ordinal in 0..count {
        let desc = SubscriptionDesc {
            selectors: vec![SelectorDesc {
                type_selection: type_selection.clone(),
                id: None,
                id_pattern: None,
            }],
            triggers: vec![NotificationTrigger::EntityCreated],
            watched: None,
        };
        index.insert(Arc::new(build_subscription(ordinal, &desc)?));
    }
    Ok(index)
}

/// The routed and the matched count of the creation of an Entity of `types`.
fn routed_and_matched(index: &CandidateIndex, types: &[usize]) -> Result<(usize, usize), SubscriptionError> {
    let event = build_event(&EventDesc {
        types: types.to_vec(),
        id: 0,
        lifecycle: EntityLifecycle::Created,
        instances: Vec::new(),
    })?;
    let funnel = index.find_matching(&event).funnel;
    Ok((funnel.routed, funnel.matched))
}

/// Clause 4.17: `Building|Sensor` selects an Entity carrying either type, so a
/// change of a Vehicle routes none of the Subscriptions and a change of either
/// type routes and matches every one.
#[test]
fn a_disjunction_routes_only_the_changes_of_its_types() -> Result<(), SubscriptionError> {
    let index = index_on(&TypeDesc::Alternatives(vec![vec![BUILDING], vec![SENSOR]]), 1000)?;
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[VEHICLE])?, (0, 0));
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[BUILDING])?, (1000, 1000));
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[SENSOR])?, (1000, 1000));
    Ok(())
}

/// Clause 4.17: `(Building;Sensor)` selects an Entity carrying both types. The
/// index routes a change of a Building, which may carry both, and the matcher
/// alone rejects a Building that is no Sensor.
#[test]
fn a_conjunction_is_routed_by_one_of_its_types_and_decided_by_the_matcher() -> Result<(), SubscriptionError> {
    let index = index_on(&TypeDesc::Alternatives(vec![vec![BUILDING, SENSOR]]), 1)?;
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[BUILDING])?, (1, 0));
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[BUILDING, SENSOR])?, (1, 1));
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[SENSOR, VEHICLE])?, (0, 0));
    Ok(())
}

/// Clause 4.17: `*` selects every entity type, so every change routes it.
#[test]
fn the_wildcard_type_routes_every_change() -> Result<(), SubscriptionError> {
    let index = index_on(&TypeDesc::Wildcard, 1)?;
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[VEHICLE])?, (1, 1));
    pretty_assertions::assert_eq!(routed_and_matched(&index, &[])?, (1, 1));
    Ok(())
}
