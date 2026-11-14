//! A corpus of `q` terms asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.9 gives a Query Term one meaning wherever a broker evaluates it, and a broker may evaluate it in more than
//! one place: in its store for a query, and over the Entity a write produced for a Subscription. A corpus states once
//! which Entities each term selects and expects both answers to be exactly those, so neither reading can drift from
//! the clause without the other noticing.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    outcome::Done,
};
use andromeda_q_corpus::corpus::Traversal;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityId, EntityTypeName, Input, Lossless, NgsiLdMediaType},
    query::{Join, q::Query, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::{collections::HashSet, num::NonZeroU32, time::Duration};
use tokio::time::sleep;

/// How long every endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// The request members a term is asked with beside `q`.
#[derive(Debug, Clone, Copy)]
pub struct QRequest<'c> {
    /// The Attributes whose operands are expanded against the `@context` (`expandValues`).
    pub expand_values: &'c [&'c str],
    /// The Attributes whose compound members stay literal (`jsonKeys`).
    pub json_keys: &'c [&'c str],
    /// Whether the term reads through a Relationship, which Query Entities admits only with `join` and `joinLevel`
    /// (clause 5.7.2.4) and a Subscription with no bound.
    pub traversal: Traversal,
}

impl QRequest<'_> {
    /// A term read over the Entity's own Attributes, with no `expandValues` or `jsonKeys`.
    pub const PLAIN: QRequest<'static> = QRequest {
        expand_values: &[],
        json_keys: &[],
        traversal: Traversal::Local,
    };
}

/// One Query Term of a corpus and the Entities clause 4.9 has it select.
#[derive(Debug, Clone, Copy)]
pub struct QCase<'c> {
    /// The term, as a client writes it.
    pub q: &'c str,
    /// The Entities it selects among those the corpus creates, in any order.
    pub selects: &'c [&'c EntityId],
    /// The members the term is asked with beside `q`.
    pub request: QRequest<'c>,
}

/// Subscribes to the Entities of `entity_type` once for each case, with its `q`, creates `entities`, and expects Query
/// Entities by `entity_type` with each `q`, and the notifications each Subscription receives, to name exactly the
/// Entities the case selects.
///
/// Every Subscription exists before the first Entity is created, so each creation is matched against every term. A
/// notification owed but never delivered shows as a missing Entity in the comparison, after the delivery deadline.
///
/// # Errors
///
/// Fails when a value cannot be built, a term is not a query, a request is refused or the sink cannot be read.
pub async fn expect_query_and_subscription_alike(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    entities: &[Entity<Input>],
    cases: &[QCase<'_>],
) -> Result<()> {
    let sink = NotificationSink::start().await?;
    let mut subscription_ids = Vec::with_capacity(cases.len());
    for (endpoint, case) in cases.iter().enumerate() {
        let subscription_id = namespace.subscription_id(format!("q-{endpoint}"))?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([entity_type.as_str()])
            .q(case.q.parse::<Query>()?)
            .expand_values(case.request.expand_values.iter().copied())
            .json_keys(case.request.json_keys.iter().copied())
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
        subscription_ids.push(subscription_id);
    }
    for entity in entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for case in cases {
        let queried = queried_ids(broker, entity_type, case).await?;
        expect_that!(queried, eq(&selected(case)), "Query Entities with q={}", case.q);
    }
    for (endpoint, case) in cases.iter().enumerate() {
        match sink.wait_for_at_least(endpoint, case.selects.len()).await {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
    }
    sleep(QUIET_WINDOW).await;
    for (endpoint, case) in cases.iter().enumerate() {
        let mut notified = HashSet::new();
        for notification in sink.received(endpoint).await? {
            notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.clone()));
        }
        expect_that!(notified, eq(&selected(case)), "a Subscription with q={}", case.q);
    }
    for subscription_id in &subscription_ids {
        broker.subscriptions().delete(subscription_id).send().await?;
    }
    Ok(())
}

/// Subscribes to the Entities of `entity_type` once for each of `filters`, creates `entities`, and expects the
/// notifications of each Subscription to name the Entities Query Entities answers for its filter, whichever those
/// are.
///
/// One Subscription's notifications are not delivered in the order of the writes that raised them, so no wait can
/// prove a notification will never come. The comparison waits for as many notifications as Query Entities answered
/// Entities, then compares what arrived both ways: an Entity Query Entities answers and no notification named, and
/// an Entity a notification named that Query Entities did not answer. A notification of an unselected Entity that
/// arrives later is not seen; the fixed corpus states those Entities instead.
///
/// # Errors
///
/// Fails when a value cannot be built, a filter is not a query, a request is refused or the sink cannot be read.
pub async fn expect_query_and_subscription_to_agree(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    entities: &[Entity<Input>],
    filters: &[String],
) -> Result<()> {
    let sink = NotificationSink::start().await?;
    for (endpoint, filter) in filters.iter().enumerate() {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(format!("drawn-{endpoint}"))?)
            .entities([entity_type.as_str()])
            .q(filter.parse::<Query>()?)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    for entity in entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for (endpoint, filter) in filters.iter().enumerate() {
        let case = QCase {
            q: filter,
            selects: &[],
            request: QRequest::PLAIN,
        };
        let queried = queried_ids(broker, entity_type, &case).await?;
        match sink.wait_for_at_least(endpoint, queried.len()).await {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
        let mut notified = HashSet::new();
        for notification in sink.received(endpoint).await? {
            notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.clone()));
        }
        expect_that!(notified, eq(&queried), "a Subscription and Query Entities with q={filter}");
    }
    Ok(())
}

/// The Entities of `entity_type` that Query Entities answers for `case`.
///
/// A term reading through a Relationship is asked with `join=inline`, which nests every Linked Entity inside the
/// Relationship that names it, so the answer lists the selected Entities alone (clause 4.5.23.2).
async fn queried_ids(broker: &BrokerClient, entity_type: &EntityTypeName, case: &QCase<'_>) -> Result<HashSet<EntityId>> {
    let (join, join_level) = match case.request.traversal {
        Traversal::Local => (None, None),
        Traversal::OneHop => (Some(Join::Inline), Some(NonZeroU32::MIN)),
    };
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(entity_type).build()?)
        .q(case.q.parse::<Query>()?)
        .expand_values(names(case.request.expand_values)?)
        .json_keys(names(case.request.json_keys)?)
        .maybe_join(join)
        .maybe_join_level(join_level)
        .send()
        .await?;
    Ok(page.items().iter().map(|entity| entity.id.clone()).collect())
}

/// The attribute names `terms`.
fn names(terms: &[&str]) -> Result<Vec<AttributeName>> {
    let mut names = Vec::with_capacity(terms.len());
    for term in terms {
        names.push(term.parse()?);
    }
    Ok(names)
}

/// The Entities `case` selects.
fn selected(case: &QCase<'_>) -> HashSet<EntityId> {
    case.selects.iter().map(|id| (*id).clone()).collect()
}
