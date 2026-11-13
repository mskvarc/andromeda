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
    q_selection::selected_ids,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, EntityTypeName, Input, Lossless, NgsiLdMediaType},
    query::q::Query,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::{collections::HashSet, time::Duration};
use tokio::time::sleep;

/// How long every endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// One Query Term of a corpus and the Entities clause 4.9 has it select.
#[derive(Debug, Clone, Copy)]
pub struct QCase<'c> {
    /// The term, as a client writes it.
    pub q: &'c str,
    /// The Entities it selects among those the corpus creates, in any order.
    pub selects: &'c [&'c EntityId],
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
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
        subscription_ids.push(subscription_id);
    }
    for entity in entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for case in cases {
        let queried: HashSet<EntityId> = selected_ids(broker, entity_type, case.q).await?.into_iter().collect();
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

/// The Entities `case` selects.
fn selected(case: &QCase<'_>) -> HashSet<EntityId> {
    case.selects.iter().map(|id| (*id).clone()).collect()
}
