//! Generated filters asked of Query Entities and of Subscriptions, with no answer stated for them.
//!
//! A generated case states no selection, so the two executions are held to each other. Each filter is asked twice:
//! of Subscriptions that exist before the Entities are created, and of Subscriptions that exist before a second write
//! changes each Entity, so the in-memory evaluation is tested over created and over changed Entities. Each time, the
//! Entities a Subscription is notified of are the Entities Query Entities answers for its filter. Both rounds write
//! the Entities in position order, so an Entity a Relationship names at a lower position is written first. What the
//! filters selected in each round is reported for the bounds a generated test holds its generator to.

use crate::{
    broker::BrokerClient,
    drawn_selection::{Moment, RoundSelection, Selections},
    filter_parity::{Filter, QUIET_WINDOW, notified_ids, queried_ids, subscribe},
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, EntityTypeName, Input},
    subscription::SubscriptionId,
};
use tokio::time::sleep;

/// One filter asked of Query Entities and of a Subscription with no answer stated for it.
#[derive(Debug, Clone)]
pub(crate) struct LabelledFilter<'c> {
    /// How the filter is named in a failure.
    pub(crate) label: String,
    /// The filter.
    pub(crate) filter: Filter<'c>,
}

/// The writes of a generated case.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DrawnWrites<'w> {
    /// The Entities, created first.
    pub(crate) entities: &'w [Entity<Input>],
    /// What Append Entity Attributes then adds to the Entity at the same position.
    pub(crate) changes: &'w [EntityFragment],
}

/// The sink endpoint the Subscription for the filter at `position` of `count` notifies at `moment`.
const fn sink_endpoint(moment: Moment, count: usize, position: usize) -> usize {
    match moment {
        Moment::Created => position,
        Moment::Changed => count + position,
    }
}

/// Creates the Entities of `writes` and then changes each one, and expects the notifications of a Subscription with
/// each of `filters`, made before each of the two rounds of writes, to name the Entities Query Entities answers for
/// that filter after the round, whichever those are. A failure names the filter and the round, and each Entity only
/// one of the two named, with what `described` says of the Entity at its position in the writes. Answers what Query
/// Entities selected in each round.
///
/// # Errors
///
/// Fails when the writes do not pair one change with each Entity, a request member cannot be built, a request is
/// refused or the sink cannot be read.
pub(crate) async fn expect_agreement(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    writes: DrawnWrites<'_>,
    filters: &[LabelledFilter<'_>],
    described: impl Fn(usize) -> Option<String> + Send + Sync,
) -> Result<Selections> {
    if writes.changes.len() != writes.entities.len() {
        fail!("{} changes for {} entities", writes.changes.len(), writes.entities.len())?;
    }
    let sink = NotificationSink::start().await?;
    let created = subscribe_each(broker, namespace, entity_type, &sink, filters, Moment::Created).await?;
    for entity in writes.entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    let after_creation = expect_answers_alike(broker, entity_type, &sink, filters, Moment::Created, writes.entities, &described).await?;
    unsubscribe(broker, &created).await?;
    let changed = subscribe_each(broker, namespace, entity_type, &sink, filters, Moment::Changed).await?;
    for (entity, change) in writes.entities.iter().zip(writes.changes) {
        broker.entities().append_attributes(&entity.id, change).send().await?.done()?;
    }
    let after_change = expect_answers_alike(broker, entity_type, &sink, filters, Moment::Changed, writes.entities, &described).await?;
    unsubscribe(broker, &changed).await?;
    Ok(Selections::new(after_creation, after_change))
}

/// Subscribes to the Entities of `entity_type` once for each of `filters`, each notifying its endpoint for `moment`.
///
/// # Errors
///
/// Fails when a Subscription cannot be built or is refused.
async fn subscribe_each(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    sink: &NotificationSink,
    filters: &[LabelledFilter<'_>],
    moment: Moment,
) -> Result<Vec<SubscriptionId>> {
    let mut subscriptions = Vec::with_capacity(filters.len());
    for (position, filter) in filters.iter().enumerate() {
        let endpoint = sink_endpoint(moment, filters.len(), position);
        subscriptions.push(subscribe(broker, namespace, entity_type, sink, endpoint, "drawn", &filter.filter).await?);
    }
    Ok(subscriptions)
}

/// Deletes `subscriptions`, so the next round of writes notifies only the Subscriptions made for it.
///
/// # Errors
///
/// Fails when a delete is refused.
async fn unsubscribe(broker: &BrokerClient, subscriptions: &[SubscriptionId]) -> Result<()> {
    for subscription in subscriptions {
        broker.subscriptions().delete(subscription).send().await?;
    }
    Ok(())
}

/// Expects the notifications each Subscription of `moment` received to name the Entities Query Entities answers for
/// its filter now, and answers what Query Entities selected.
///
/// One Subscription's notifications are not delivered in the order of the writes that raised them, so no count of
/// them proves the last has come. Each endpoint is awaited until it holds as many notifications as Query Entities
/// answered Entities, every endpoint is then watched for the same quiet window as a corpus case, and only then are
/// both directions compared: an Entity Query Entities answers and no notification named, and an Entity a
/// notification named that Query Entities did not answer.
///
/// # Errors
///
/// Fails when a request member cannot be built, a query is refused or the sink cannot be read.
async fn expect_answers_alike(
    broker: &BrokerClient,
    entity_type: &EntityTypeName,
    sink: &NotificationSink,
    filters: &[LabelledFilter<'_>],
    moment: Moment,
    entities: &[Entity<Input>],
    described: &(impl Fn(usize) -> Option<String> + Sync),
) -> Result<RoundSelection> {
    let mut answers = Vec::with_capacity(filters.len());
    for (position, filter) in filters.iter().enumerate() {
        let queried = queried_ids(broker, entity_type, &filter.filter).await?;
        match sink.wait_for_at_least(sink_endpoint(moment, filters.len(), position), queried.len()).await {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
        answers.push(queried);
    }
    sleep(QUIET_WINDOW).await;
    for (position, (filter, queried)) in filters.iter().zip(&answers).enumerate() {
        let notified = notified_ids(sink, sink_endpoint(moment, filters.len(), position)).await?;
        let one_sided = queried
            .symmetric_difference(&notified)
            .map(|id| described_entity(entities, id, described))
            .collect::<Vec<_>>()
            .join("; ");
        expect_that!(
            notified,
            eq(queried),
            "a Subscription and Query Entities {moment} with {}; named by one of them alone: {one_sided}",
            filter.label
        );
    }
    Ok(RoundSelection::of(&answers, entities.len()))
}

/// The identifier `id`, with what `described` says of the Entity of `entities` it identifies.
fn described_entity(entities: &[Entity<Input>], id: &EntityId, described: &impl Fn(usize) -> Option<String>) -> String {
    entities
        .iter()
        .position(|entity| entity.id == *id)
        .and_then(described)
        .map_or_else(|| id.to_string(), |drawn| format!("{id} {drawn}"))
}

/// The identifiers of `count` drawn Entities under `namespace`, each named for its position.
///
/// # Errors
///
/// Fails when an identifier cannot be built.
pub(crate) fn drawn_ids(namespace: &Namespace, count: usize) -> Result<Vec<EntityId>> {
    let mut ids = Vec::with_capacity(count);
    for position in 0..count {
        ids.push(namespace.urn("Drawn", position)?);
    }
    Ok(ids)
}
