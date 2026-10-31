//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Merge Entity addressed to an id no entity has is refused with 404 `ResourceNotFound` and creates nothing
/// Clause: 5.6.17.4, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0008_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let fragment = EntityFragment::for_entity(building_id.clone(), building_type)
        .property("name", "Pisa Tower")
        .build()?;
    let answer = broker.with_responses().entities().merge(&building_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
