//! How long a `q` filter takes to evaluate over one Entity, and how long one
//! change takes to match against many Subscriptions of one entity type.
//!
//! The in-memory reading of clause 4.9 runs once per candidate Subscription
//! per change, so these measure the costs a change of it moves: one filter
//! over one Entity of twenty Attributes, one change against a thousand to a
//! hundred thousand Subscriptions sharing ten filters, and one change against
//! Subscriptions whose filters each need an Attribute the Entity lacks.

use andromeda_context_commit::{
    change_event::ChangeEvent,
    store_instant::StoreInstant,
    subscription_key::{SubscriptionKey, SubscriptionKeyOutOfRange},
};
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    reads::{
        attribute_change::{AttributeChange, AttributeChangeKind},
        attribute_instance_key::AttributeInstanceKey,
    },
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    notification::{ChangeOrigin, NotificationTrigger},
    subscription::{SubscriptionStatus, entity_selector::EntitySelector, standing::SubscriptionStanding},
};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{eval::evaluate, expression::Expr, language_matching::LanguageMatching, parser::parse},
    projection::compiled::Projection,
    queried_entity::linked_entities::NoLinkedEntities,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{
    candidate_index::CandidateIndex,
    compiled::CompiledSubscription,
    geo_filter::SubscriptionGeoFilter,
    matching::matches_entity_filters,
    notifying_since::NotifyingSince,
};
use chrono::{DateTime, Utc};
use criterion::{BenchmarkId, Criterion};
use iri_rs::{Iri, iri};
use std::{hint::black_box, num::NonZeroU32, sync::Arc};
use url::Url;

/// The vocabulary every benchmark attribute is a term of.
const VOCABULARY: Iri<&'static str> = iri!("https://example.com/");

/// The entity type every benchmark Entity and Subscription has.
const ENTITY_TYPE: Iri<&'static str> = iri!("https://example.com/Vehicle");

/// A Vehicle of twenty Attributes, numbers, strings and Booleans, `a9`
/// holding a sub-property, as expanded JSON-LD.
const VEHICLE: &[u8] = include_bytes!("vehicle.jsonld");

/// Ten filters spanning the operators and the attribute families, each over
/// an Attribute the benchmark Entity holds.
const FILTERS: [&str; 10] = [
    "a0==10",
    "a1!=\"x\"",
    "a2>5;a3<100",
    "a4==1..50|a5",
    "a1~=\"^v.*\"",
    "a5!~=\"z\"",
    "a6==1,2,3,8",
    "a9.s0==\"sub\"",
    "a7==true;a11!=false",
    "(a12>1|a13<0);a13==\"v13\"",
];

/// Why a benchmark could not be set up.
#[derive(Debug, thiserror::Error)]
enum SetupError {
    /// A filter is not a filter.
    #[error("a filter does not parse")]
    Filter(#[from] QueryError),
    /// The Entity does not decode.
    #[error("the entity does not decode")]
    Entity(#[from] EntityRefusal),
    /// A name is not a name.
    #[error("a name is not valid")]
    Name(#[from] NgsiLdReferenceError),
    /// An identifier is not an identifier.
    #[error("an identifier is not valid")]
    Identifier(#[from] IdentifierParseError),
    /// A Subscription key is out of range.
    #[error("a subscription key is out of range")]
    Key(#[from] SubscriptionKeyOutOfRange),
    /// The endpoint is not a URL.
    #[error("the endpoint is not a URL")]
    Endpoint(#[from] url::ParseError),
}

/// The `q` text `filter` with every attribute name written as an expanded
/// IRI.
fn expanded(filter: &str) -> Result<Expr, QueryError> {
    let mut text = String::with_capacity(filter.len() * 4);
    let mut quoted = false;
    let mut name = String::new();
    for character in filter.chars().chain(['\0']) {
        if !quoted && (character.is_ascii_alphanumeric() || character == '_') && (!name.is_empty() || character.is_ascii_alphabetic()) {
            name.push(character);
            continue;
        }
        if !name.is_empty() {
            if name == "true" || name == "false" {
                text.push_str(&name);
            } else {
                text.extend(['"']);
                text.push_str(VOCABULARY.as_str());
                text.push_str(&name);
                text.extend(['"']);
            }
            name.clear();
        }
        if character == '"' {
            quoted = !quoted;
        }
        if character != '\0' {
            text.push(character);
        }
    }
    parse(&text)
}

/// The creation of the Vehicle.
fn creation() -> Result<ChangeEvent, SetupError> {
    Ok(ChangeEvent::builder()
        .entity_id("urn:ngsi-ld:Vehicle:1".parse()?)
        .entity_types(vec![ENTITY_TYPE.as_str().parse()?])
        .change(EntityChange::new(
            EntityLifecycle::Created,
            vec![AttributeChange {
                key: AttributeInstanceKey {
                    attr: format!("{}a0", VOCABULARY.as_str()).parse()?,
                    dataset_id: None,
                },
                kind: AttributeChangeKind::Created,
            }],
        ))
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
        .build())
}

/// A Subscription to every Vehicle, keyed `key`, filtering on `q`.
fn subscription(key: i64, q: Expr) -> Result<Arc<CompiledSubscription>, SetupError> {
    Ok(Arc::new(CompiledSubscription {
        id: format!("urn:ngsi-ld:Subscription:{key}").parse()?,
        key: SubscriptionKey::try_from(key)?,
        standing: SubscriptionStanding {
            status: SubscriptionStatus::Active,
            expires_at: None,
        },
        endpoint_uri: Url::parse("http://example.com/notify")?,
        endpoint_accept: None,
        endpoint_receiver_info: None,
        endpoint_notifier_info: None,
        endpoint_cooldown: None,
        endpoint_timeout: None,
        triggers: vec![NotificationTrigger::EntityCreated],
        watched_attrs: None,
        selectors: vec![EntitySelector {
            id: None,
            id_pattern: None,
            type_: ENTITY_TYPE.as_str().parse()?,
        }],
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
        compiled_q: Some(q),
        language_matching: LanguageMatching::TagEquality,
        compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
        compiled_scope_q: None,
        dataset_id: None,
        notification_join: None,
        notification_join_level: NonZeroU32::MIN,
        compiled_csf: None,
        local_only: false,
        ngsild_conformance: None,
        lang: None,
        notification_projection: Projection::default(),
    }))
}

/// An index of `count` Subscriptions whose filters cycle through `filters`.
fn index_of(count: i64, filters: &[Expr]) -> Result<CandidateIndex, SetupError> {
    let mut index = CandidateIndex::default();
    for (key, filter) in (1..=count).zip(filters.iter().cycle()) {
        index.insert(subscription(key, filter.clone())?);
    }
    Ok(index)
}

/// What every benchmark reads.
struct Fixture {
    /// The Vehicle every filter is evaluated over.
    entity: EntityState,
    /// The creation of the Vehicle.
    change: ChangeEvent,
    /// The ten filters, expanded.
    filters: Vec<Expr>,
    /// Ten filters each needing an Attribute the Vehicle lacks.
    absent: Vec<Expr>,
}

impl Fixture {
    /// Build what every benchmark reads.
    fn new() -> Result<Fixture, SetupError> {
        Ok(Fixture {
            entity: EntityState::from_slice(VEHICLE)?,
            change: creation()?,
            filters: FILTERS.iter().map(|filter| expanded(filter)).collect::<Result<_, _>>()?,
            absent: (0..10)
                .map(|position| expanded(&format!("a{position}==1;absent{position}>0")))
                .collect::<Result<_, _>>()?,
        })
    }

    /// The Subscriptions `index` holds that a change of the Vehicle notifies.
    fn notified(&self, index: &CandidateIndex) -> usize {
        index
            .find_matching(&self.change)
            .matched
            .iter()
            .filter(|subscription| matches_entity_filters(subscription, &self.entity, NoLinkedEntities))
            .count()
    }
}

/// B-1: each filter evaluated over the Vehicle.
fn one_filter_over_one_entity(criterion: &mut Criterion, fixture: &Fixture) {
    let mut group = criterion.benchmark_group("evaluate");
    for (position, expr) in fixture.filters.iter().enumerate() {
        group.bench_with_input(BenchmarkId::new("filter", position), expr, |bencher, expr| {
            bencher.iter(|| evaluate(black_box(expr), &fixture.entity, NoLinkedEntities, LanguageMatching::TagEquality));
        });
    }
    group.finish();
}

/// B-2: one change against many Subscriptions sharing ten filters.
fn one_change_against_many_subscriptions(criterion: &mut Criterion, fixture: &Fixture) -> Result<(), SetupError> {
    let mut group = criterion.benchmark_group("match");
    group.sample_size(10);
    for count in [1_000, 10_000, 100_000] {
        let index = index_of(count, &fixture.filters)?;
        group.bench_with_input(BenchmarkId::new("subscriptions", count), &index, |bencher, index| {
            bencher.iter(|| fixture.notified(black_box(index)));
        });
    }
    group.finish();
    Ok(())
}

/// B-3: one change against Subscriptions whose filters each need an
/// Attribute the Vehicle lacks.
fn one_change_against_subscriptions_needing_an_absent_attribute(criterion: &mut Criterion, fixture: &Fixture) -> Result<(), SetupError> {
    let index = index_of(10_000, &fixture.absent)?;
    let mut group = criterion.benchmark_group("match-absent");
    group.sample_size(10);
    group.bench_function("subscriptions/10000", |bencher| bencher.iter(|| fixture.notified(black_box(&index))));
    group.finish();
    Ok(())
}

/// Run every benchmark, or fail with the reason the fixture cannot be built.
fn main() -> Result<(), SetupError> {
    let fixture = Fixture::new()?;
    let mut criterion = Criterion::default().configure_from_args();
    one_filter_over_one_entity(&mut criterion, &fixture);
    one_change_against_many_subscriptions(&mut criterion, &fixture)?;
    one_change_against_subscriptions_needing_an_absent_attribute(&mut criterion, &fixture)?;
    criterion.final_summary();
    Ok(())
}
