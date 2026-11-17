//! A corpus of geoqueries asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.10 gives a geoquery one meaning wherever a broker evaluates it, and a broker may evaluate it in more than
//! one place: in its store for a query, and over the Entity a write produced for a Subscription. A corpus states once
//! which Entities each geoquery selects and expects both answers to be exactly those, so neither reading can drift
//! from the clause without the other noticing.

use crate::{
    broker::BrokerClient,
    filter_parity::{Filter, ParityCase, expect_alike},
    namespace::Namespace,
    q_parity::QRequest,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, EntityTypeName, Input},
    query::geoquery::GeoQuery,
};

/// One geoquery of a corpus and the Entities clause 4.10 has it select.
#[derive(Debug, Clone, Copy)]
pub struct GeoCase<'c> {
    /// How the geoquery is named in a failure.
    pub label: &'c str,
    /// The geoquery.
    pub geo_q: &'c GeoQuery,
    /// The Entities it selects among those the corpus creates, in any order.
    pub selects: &'c [&'c EntityId],
}

/// Subscribes to the Entities of `entity_type` once for each case, with its geoquery as `geoQ`, creates `entities`,
/// and expects Query Entities by `entity_type` with each geoquery, and the notifications each Subscription receives,
/// to name exactly the Entities the case selects.
///
/// # Errors
///
/// Fails when a value cannot be built, a request is refused or the sink cannot be read.
pub async fn expect_geo_query_and_subscription_alike(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    entities: &[Entity<Input>],
    cases: &[GeoCase<'_>],
) -> Result<()> {
    let parity_cases: Vec<ParityCase<'_>> = cases
        .iter()
        .map(|case| ParityCase {
            label: case.label.to_owned(),
            filter: Filter {
                q: None,
                geo_q: Some(case.geo_q.clone()),
                request: QRequest::PLAIN,
            },
            selects: case.selects,
        })
        .collect();
    expect_alike(broker, namespace, entity_type, entities, &parity_cases).await
}
