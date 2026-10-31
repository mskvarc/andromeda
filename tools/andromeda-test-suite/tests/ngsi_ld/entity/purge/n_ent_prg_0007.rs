//! A selector matching nothing answers 204.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::query::Locality;

/// Pins: Purge Entities with a type selector no entity has answers 204 with no body
/// Clause: 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_0007_a_selector_matching_nothing_answers_204() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&namespace.entity_type("Nothing")?)
        .locality(Locality::Local)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(received.body.len(), eq(0));
    Ok(())
}
