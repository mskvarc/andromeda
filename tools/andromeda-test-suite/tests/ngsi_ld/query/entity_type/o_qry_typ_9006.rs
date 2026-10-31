//! Whitespace in a type selection is skipped.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    raw_call::{call, target},
    unchecked_page::entities,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use ngsi_ld::model::{Entity, EntityId};

/// Query Entities by GET with the type selection text `selection`, as written.
async fn get_things(broker: &BrokerClient, selection: &str) -> Result<Response<Bytes>> {
    // The type selection of the model is built from names, so spaced text is sent as written.
    Ok(call(
        broker,
        Method::GET,
        &target("entities", &[("type", selection)]),
        &[("Accept", "application/json")],
        None,
    )
    .await?)
}

/// Checks that the answer is a 200 holding exactly the entities `ids`.
fn expect_exactly(response: &Response<Bytes>, ids: &[&EntityId]) -> Result<()> {
    verify_that!(response.status(), eq(StatusCode::OK))?;
    let found = entities(response)?;
    expect_that!(found, len(eq(ids.len())));
    for id in ids {
        expect_that!(found, contains(has_id(*id)));
    }
    Ok(())
}

/// Pins: spaces around the names, operators and parentheses of an Entity Type Selection are skipped, and the selection
/// answers what its unspaced form does
/// Clause: 4.17, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn o_qry_typ_9006_whitespace_in_a_type_selection_is_skipped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let home_vehicle_id = namespace.urn("Thing", "home-vehicle")?;
    let home_id = namespace.urn("Thing", "home")?;
    let motorhome_id = namespace.urn("Thing", "motorhome")?;
    let home_type = namespace.entity_type("Home")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let motorhome_type = namespace.entity_type("Motorhome")?;
    let entities = vec![
        Entity::builder()
            .id(home_vehicle_id.clone())
            .entity_type(vec![home_type.clone(), vehicle_type.clone()])
            .build()?,
        Entity::builder().id(home_id.clone()).entity_type(home_type.clone()).build()?,
        Entity::builder().id(motorhome_id.clone()).entity_type(motorhome_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let spaced = get_things(&broker, &format!("{motorhome_type} | ( {home_type} ; {vehicle_type} )")).await?;
    expect_exactly(&spaced, &[&home_vehicle_id, &motorhome_id])?;
    let unspaced = get_things(&broker, &format!("({home_type};{vehicle_type})|{motorhome_type}")).await?;
    expect_exactly(&unspaced, &[&home_vehicle_id, &motorhome_id])?;
    let listed = get_things(&broker, &format!("{home_type} , {motorhome_type}")).await?;
    expect_exactly(&listed, &[&home_vehicle_id, &home_id, &motorhome_id])?;
    Ok(())
}
