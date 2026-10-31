//! A query body spelled with at type selects the entities.

use andromeda_test_suite::{
    broker,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities by POST whose Query and entity selector types are written as @type answers 200 with the
/// Entity of that type
/// Clause: 4.4, 5.2.23, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_9008_a_query_body_spelled_with_at_type_selects_the_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "queried")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The Query document is spelled with the JSON-LD keyword `@type`, which the model does not write.
    let body = format!(r#"{{"@type": "Query", "entities": [{{"@type": "Building_{namespace}"}}]}}"#);
    let request = exchange(
        &broker,
        Method::POST,
        "entityOperations/query",
        &[("Content-Type", "application/json")],
        Some(body.as_str()),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    expect_that!(entities(&answer?)?.as_slice(), ids_in_order(&[building_id]));
    Ok(())
}
