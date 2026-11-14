//! Generated filters select alike in a query and a subscription.

use andromeda_test_suite::{broker, namespace::Namespace, q_generation::GeneratedCase, q_parity::expect_query_and_subscription_to_agree};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::model::EntityId;

/// The seeds the cases are drawn from, each drawing the same case on every run.
const SEEDS: u64 = 64;

/// The Entities each seed draws.
const ENTITIES: usize = 20;

/// The filters each seed draws.
const FILTERS: usize = 30;

/// Creates the Entities `seed` draws under a namespace of its own and expects Query Entities and a Subscription with
/// each filter it draws to agree on the Entities selected.
async fn expect_drawn_case_to_agree(seed: u64) -> Result<()> {
    let case = GeneratedCase::drawn(seed, ENTITIES, FILTERS)?;
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity_type = namespace.entity_type("Drawn")?;
    let dataset = namespace.dataset_id("Dataset", 1)?;
    let mut ids: Vec<EntityId> = Vec::with_capacity(ENTITIES);
    for position in 0..ENTITIES {
        ids.push(namespace.urn("Drawn", position)?);
    }
    let documents = case.documents(&entity_type, &dataset, &ids)?;
    expect_query_and_subscription_to_agree(&broker, &namespace, &entity_type, &documents, &case.filters).await
}

/// Pins: for 64 fixed seeds, each drawing 20 Entities over eight Attribute names of every attribute family and 30
/// filters over the same names, Query Entities and a Subscription with each filter select the same Entities; clause
/// 4.9 gives a Query Term one meaning wherever it is evaluated
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9178_generated_filters_select_alike_in_a_query_and_a_subscription() -> Result<()> {
    for outcome in join_all((0..SEEDS).map(expect_drawn_case_to_agree)).await {
        outcome?;
    }
    Ok(())
}
