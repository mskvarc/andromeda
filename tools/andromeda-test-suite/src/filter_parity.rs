//! One filter asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.9 gives a Query Term, and clause 4.10 a geoquery, one meaning wherever a broker evaluates it, and a broker
//! may evaluate it in more than one place: in its store for a query, and over the Entity a write produced for a
//! Subscription. This module asks both with the same filter and reads back the Entities each answers; the corpora of
//! [`crate::q_parity`] and [`crate::geo_parity`] state the Entities each filter selects.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    outcome::Done,
    q_parity::QRequest,
};
use andromeda_q_corpus::corpus::Traversal;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityId, EntityTypeName, Input, Lossless, NgsiLdMediaType},
    query::{Join, geoquery::GeoQuery, q::Query, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use std::{collections::HashSet, num::NonZeroU32, time::Duration};
use tokio::time::sleep;

/// How long every endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// The members a query and a Subscription are both asked with.
#[derive(Debug, Clone)]
pub(crate) struct Filter<'c> {
    /// `q`, when the filter has one.
    pub(crate) q: Option<Query>,
    /// `geoQ` for a Subscription, `georel`, `geometry`, `coordinates` and `geoproperty` for a query, when the filter
    /// has one.
    pub(crate) geo_q: Option<GeoQuery>,
    /// The request members asked beside `q`.
    pub(crate) request: QRequest<'c>,
}

/// One filter of a corpus and the Entities it selects.
#[derive(Debug, Clone)]
pub(crate) struct ParityCase<'c> {
    /// How the filter is named in a failure.
    pub(crate) label: String,
    /// The filter.
    pub(crate) filter: Filter<'c>,
    /// The Entities it selects among those the corpus creates, in any order.
    pub(crate) selects: &'c [&'c EntityId],
}

/// Subscribes to the Entities of `entity_type` once for each case, with its filter, creates `entities`, and expects
/// Query Entities by `entity_type` with each filter, and the notifications each Subscription receives, to name
/// exactly the Entities the case selects.
///
/// Every Subscription exists before the first Entity is created, so each creation is matched against every filter. A
/// notification owed but never delivered shows as a missing Entity in the comparison, after the delivery deadline.
///
/// # Errors
///
/// Fails when a value cannot be built, a request is refused or the sink cannot be read.
pub(crate) async fn expect_alike(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    entities: &[Entity<Input>],
    cases: &[ParityCase<'_>],
) -> Result<()> {
    let sink = NotificationSink::start().await?;
    let mut subscription_ids = Vec::with_capacity(cases.len());
    for (endpoint, case) in cases.iter().enumerate() {
        subscription_ids.push(subscribe(broker, namespace, entity_type, &sink, endpoint, "filter", &case.filter).await?);
    }
    for entity in entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for case in cases {
        let queried = queried_ids(broker, entity_type, &case.filter).await?;
        expect_that!(queried, eq(&selected(case.selects)), "Query Entities with {}", case.label);
    }
    for (endpoint, case) in cases.iter().enumerate() {
        match sink.wait_for_at_least(endpoint, case.selects.len()).await {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
    }
    sleep(QUIET_WINDOW).await;
    for (endpoint, case) in cases.iter().enumerate() {
        let notified = notified_ids(&sink, endpoint).await?;
        expect_that!(notified, eq(&selected(case.selects)), "a Subscription with {}", case.label);
    }
    for subscription_id in &subscription_ids {
        broker.subscriptions().delete(subscription_id).send().await?;
    }
    Ok(())
}

/// Subscribes to the Entities of `entity_type` with `filter`, notifying the sink's endpoint `endpoint`, under an
/// identifier named `prefix` and the endpoint.
///
/// # Errors
///
/// Fails when the Subscription cannot be built or is refused.
pub(crate) async fn subscribe(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    sink: &NotificationSink,
    endpoint: usize,
    prefix: &str,
    filter: &Filter<'_>,
) -> Result<SubscriptionId> {
    let subscription_id = namespace.subscription_id(format!("{prefix}-{endpoint}"))?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([entity_type.as_str()])
        .maybe_q(filter.q.clone())
        .maybe_geo_q(filter.geo_q.clone())
        .expand_values(filter.request.expand_values.iter().copied())
        .json_keys(filter.request.json_keys.iter().copied())
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    Ok(subscription_id)
}

/// The Entities of `entity_type` that Query Entities answers for `filter`.
///
/// A term reading through a Relationship is asked with `join=inline`, which nests every Linked Entity inside the
/// Relationship that names it, so the answer lists the selected Entities alone (clause 4.5.23.2).
///
/// # Errors
///
/// Fails when a request member cannot be built or the query is refused.
pub(crate) async fn queried_ids(broker: &BrokerClient, entity_type: &EntityTypeName, filter: &Filter<'_>) -> Result<HashSet<EntityId>> {
    let (join, join_level) = match filter.request.traversal {
        Traversal::Local => (None, None),
        Traversal::OneHop => (Some(Join::Inline), Some(NonZeroU32::MIN)),
    };
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(entity_type).build()?)
        .maybe_q(filter.q.clone())
        .maybe_geo_q(filter.geo_q.clone())
        .expand_values(names(filter.request.expand_values)?)
        .json_keys(names(filter.request.json_keys)?)
        .maybe_join(join)
        .maybe_join_level(join_level)
        .send()
        .await?;
    Ok(page.items().iter().map(|entity| entity.id.clone()).collect())
}

/// The Entities named by every notification the sink's endpoint `endpoint` received.
///
/// # Errors
///
/// Fails when the sink cannot be read or a notification is not one.
pub(crate) async fn notified_ids(sink: &NotificationSink, endpoint: usize) -> Result<HashSet<EntityId>> {
    let mut notified = HashSet::new();
    for notification in sink.received(endpoint).await? {
        notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.clone()));
    }
    Ok(notified)
}

/// The attribute names `terms`.
fn names(terms: &[&str]) -> Result<Vec<AttributeName>> {
    let mut names = Vec::with_capacity(terms.len());
    for term in terms {
        names.push(term.parse()?);
    }
    Ok(names)
}

/// The Entities `selects` names.
fn selected(selects: &[&EntityId]) -> HashSet<EntityId> {
    selects.iter().map(|id| (*id).clone()).collect()
}
