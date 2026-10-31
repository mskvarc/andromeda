//! Retrieve types accepting geojson is 406 listing json and json ld.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    not_acceptable::not_acceptable,
    wire::{accept, header_value},
};
use googletest::prelude::*;
use http::header::CONTENT_TYPE;
use ngsi_ld::http::operation::RetrieveEntityTypes;

/// Pins: the 406 body of a refused Accept is a JSON object whose one member, `availableRepresentations`, lists the
/// representations the resource has in order, application/json then application/ld+json
/// Clause: 6.3.4, 6.25.3.1
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations", without
/// defining its shape; Andromeda answers an object whose one member, `availableRepresentations`, holds that list
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0001_retrieve_types_accepting_geojson_is_406_listing_json_and_json_ld() -> Result<()> {
    let broker = broker::front()?;
    let request = accept(broker.unchecked(&RetrieveEntityTypes::builder().build())?, "application/geo+json");
    let response = broker.send_unchecked(request).await;
    let (head, body) = not_acceptable(&response).or_fail()?;
    expect_that!(header_value(head.headers(), &CONTENT_TYPE), some(eq("application/json")));
    expect_that!(body, json_count("$", 1));
    expect_that!(body, json_count("$.availableRepresentations", 2));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
