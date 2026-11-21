//! A corpus of `q` terms asked of Query Entities and of a Subscription over the same Entities.
//!
//! Clause 4.9 gives a Query Term one meaning wherever a broker evaluates it, and a broker may evaluate it in more than
//! one place: in its store for a query, and over the Entity a write produced for a Subscription. A corpus states once
//! which Entities each term selects and expects both answers to be exactly those, so neither reading can drift from
//! the clause without the other noticing. A generated case states no answer and expects the two answers to agree.

use crate::{
    broker::BrokerClient,
    drawn_agreement::{DrawnWrites, LabelledFilter, drawn_ids, expect_agreement},
    drawn_selection::Selections,
    filter_parity::{Filter, ParityCase, expect_alike},
    namespace::Namespace,
    q_generation::GeneratedCase,
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

/// Creates the Entities the case `seed` drew, of a type of `namespace`, then changes each of them, and expects Query
/// Entities and a Subscription with each filter it drew to select the same Entities after each round of writes,
/// whichever those are. Answers what Query Entities selected in each round.
///
/// A filter's `q` names the drawn Entities and the case's type by the identifiers and the type of `namespace`, and a
/// filter reading through a Relationship is asked of Query Entities with `join` and `joinLevel`. A failure names the
/// seed, the filter as asked with its request members and geoquery, and the drawn Attributes of each Entity only one
/// of the two selected, which is what a fixed corpus case needs to keep the disagreement covered.
///
/// # Errors
///
/// Fails when a value cannot be built, a filter is not a query, a request is refused or the sink cannot be read.
pub async fn expect_generated_case_to_agree(broker: &BrokerClient, namespace: &Namespace, seed: u64, case: &GeneratedCase) -> Result<Selections> {
    let entity_type = namespace.entity_type("Drawn")?;
    let dataset = namespace.dataset_id("Dataset", 1)?;
    let ids = drawn_ids(namespace, case.entity_count())?;
    let documents = case.documents(&entity_type, &dataset, &ids)?;
    let changes = case.changes(&dataset, &ids)?;
    let mut filters = Vec::with_capacity(case.filters().len());
    for filter in case.filters() {
        let q = filter.resolved_q(&ids, &entity_type)?;
        let geo = filter.geo.map_or_else(String::new, |part| format!(" geoQ={part}"));
        filters.push(LabelledFilter {
            label: format!(
                "seed {seed}: q={q} jsonKeys={} expandValues={}{geo}",
                filter.json_keys.join(","),
                filter.expand_values.join(",")
            ),
            filter: Filter {
                q: Some(q.parse::<Query>()?),
                geo_q: filter.geo_query()?,
                request: QRequest {
                    expand_values: &filter.expand_values,
                    json_keys: &filter.json_keys,
                    traversal: filter.traversal,
                },
            },
        });
    }
    let writes = DrawnWrites {
        entities: &documents,
        changes: &changes,
    };
    expect_agreement(broker, namespace, &entity_type, writes, &filters, |position| case.described(position)).await
}
