//! Retrieve attributes accepting xml is 406 listing json and json ld.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    not_acceptable::not_acceptable,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::http::operation::RetrieveAttributes;

/// Pins: Retrieve Available Attributes with Accept application/xml answers 406 listing the available
/// representations application/json, application/ld+json
/// Clause: 6.3.4, 5.7.8
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0016_retrieve_attributes_accepting_xml_is_406_listing_json_and_json_ld() -> Result<()> {
    let broker = broker::front()?;
    let request = accept(broker.unchecked(&RetrieveAttributes::builder().build())?, "application/xml");
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_count("$.availableRepresentations", 2));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
