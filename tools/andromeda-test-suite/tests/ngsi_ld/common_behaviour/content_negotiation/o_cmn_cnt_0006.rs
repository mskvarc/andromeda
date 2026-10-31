//! Retrieve entity accepting xml is 406 listing json json ld and geojson.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    outcome::Done,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::RetrieveEntity,
    model::{Entity, Lossless},
};

/// Pins: Retrieve Entity with Accept application/xml answers 406 listing the available representations
/// application/json, application/ld+json, application/geo+json
/// Clause: 6.3.4, 6.3.15, 5.7.1
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0006_retrieve_entity_accepting_xml_is_406_listing_json_json_ld_and_geojson() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let request = accept(
        broker.unchecked(&RetrieveEntity::builder().id(&building_id).format(Lossless::Normalized).build())?,
        "application/xml",
    );
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_count("$.availableRepresentations", 3));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    expect_that!(body, json_equals("$.availableRepresentations[2]", "application/geo+json"));
    Ok(())
}
