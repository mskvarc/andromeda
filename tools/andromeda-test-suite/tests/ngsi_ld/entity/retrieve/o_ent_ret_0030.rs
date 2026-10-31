//! Scopes read back as written in written order.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{listing_at, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Scope},
};

/// Pins: Retrieve Entity answers the Scopes an entity was created with in the spelling and the order they were
/// written, a Scope without a leading slash included
/// Clause: 4.18, 4.6.4, 5.7.1.4
/// Recorded: 4.18 defines scope as one or more `ScopePaths` and fixes no order among them; Andromeda answers them
/// in the order they were written
#[gtest]
#[tokio::test]
async fn o_ent_ret_0030_scopes_read_back_as_written_in_written_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope(["/Madrid/Gardens", "Madrid/Center", "/Avila"])
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(3)));
    expect_that!(entity.scope.as_ref(), some(listing_at(0, &"/Madrid/Gardens".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(listing_at(1, &"Madrid/Center".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(listing_at(2, &"/Avila".parse::<Scope>()?)));
    Ok(())
}
