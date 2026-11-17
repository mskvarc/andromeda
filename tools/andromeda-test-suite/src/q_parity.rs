//! A corpus of `q` terms asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.9 gives a Query Term one meaning wherever a broker evaluates it, and a broker may evaluate it in more than
//! one place: in its store for a query, and over the Entity a write produced for a Subscription. A corpus states once
//! which Entities each term selects and expects both answers to be exactly those, so neither reading can drift from
//! the clause without the other noticing.

use crate::{
    broker::BrokerClient,
    filter_parity::{Filter, ParityCase, expect_alike, notified_ids, queried_ids, subscribe},
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    outcome::Done,
};
use andromeda_q_corpus::corpus::Traversal;
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, EntityTypeName, Input},
    query::q::Query,
};

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
    let mut parity_cases = Vec::with_capacity(cases.len());
    for case in cases {
        parity_cases.push(ParityCase {
            label: format!("q={}", case.q),
            filter: Filter {
                q: Some(case.q.parse::<Query>()?),
                geo_q: None,
                request: case.request,
            },
            selects: case.selects,
        });
    }
    expect_alike(broker, namespace, entity_type, entities, &parity_cases).await
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
    let mut drawn = Vec::with_capacity(filters.len());
    for (endpoint, filter) in filters.iter().enumerate() {
        let drawn_filter = Filter {
            q: Some(filter.parse::<Query>()?),
            geo_q: None,
            request: QRequest::PLAIN,
        };
        subscribe(broker, namespace, entity_type, &sink, endpoint, "drawn", &drawn_filter).await?;
        drawn.push(drawn_filter);
    }
    for entity in entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    for (endpoint, (filter, drawn_filter)) in filters.iter().zip(&drawn).enumerate() {
        let queried = queried_ids(broker, entity_type, drawn_filter).await?;
        match sink.wait_for_at_least(endpoint, queried.len()).await {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
        let notified = notified_ids(&sink, endpoint).await?;
        expect_that!(notified, eq(&queried), "a Subscription and Query Entities with q={filter}");
    }
    Ok(())
}
