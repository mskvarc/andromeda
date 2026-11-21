//! A corpus of geoqueries asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.10 gives a geoquery one meaning wherever a broker evaluates it, and a broker may evaluate it in more than
//! one place: in its store for a query, and over the Entity a write produced for a Subscription. A corpus states once
//! which Entities each geoquery selects and expects both answers to be exactly those, so neither reading can drift
//! from the clause without the other noticing. A generated case states no answer and expects the two answers to
//! agree.

use crate::{
    broker::BrokerClient,
    drawn_agreement::{DrawnWrites, LabelledFilter, drawn_ids, expect_agreement},
    drawn_selection::Selections,
    filter_parity::{Filter, ParityCase, expect_alike},
    geo_generation::GeneratedGeoCase,
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

/// Creates the Entities the case `seed` drew, of a type of `namespace`, then moves each of them, and expects Query
/// Entities and a Subscription with each geoquery it drew as `geoQ` to select the same Entities after each round of
/// writes, whichever those are. Answers what Query Entities selected in each round.
///
/// A failure names the seed and the drawn geoquery, and the drawn `GeoProperty` Attributes of each Entity only one of
/// the two selected, which is what a fixed corpus case needs to keep the disagreement covered.
///
/// # Errors
///
/// Fails when a geometry cannot be built, a request is refused or the sink cannot be read.
pub async fn expect_generated_geo_case_to_agree(broker: &BrokerClient, namespace: &Namespace, seed: u64, case: &GeneratedGeoCase) -> Result<Selections> {
    let entity_type = namespace.entity_type("Drawn")?;
    let dataset = namespace.dataset_id("Dataset", 1)?;
    let documents = case.documents(&entity_type, &dataset, &drawn_ids(namespace, case.entity_count())?)?;
    let changes = case.changes()?;
    let mut filters = Vec::with_capacity(case.queries().len());
    for query in case.queries() {
        filters.push(LabelledFilter {
            label: format!("seed {seed}: {query:?}"),
            filter: Filter {
                q: None,
                geo_q: Some(query.geo_query()?),
                request: QRequest::PLAIN,
            },
        });
    }
    let writes = DrawnWrites {
        entities: &documents,
        changes: &changes,
    };
    expect_agreement(broker, namespace, &entity_type, writes, &filters, |position| case.described(position)).await
}
