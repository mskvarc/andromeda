//! An entity refusing xml lists all three representations as json.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    wire::{accept, header_value},
};
use googletest::prelude::*;
use http::header::CONTENT_TYPE;
use ngsi_ld::{http::operation::RetrieveEntity, model::Lossless};

/// Pins: Retrieve Entity accepting application/xml answers 406 as application/json with a body holding only
/// `availableRepresentations`: application/json, application/ld+json, application/geo+json
/// Clause: 6.3.4, 6.3.3
/// Recorded: clause 6.3.4 requires the 406 body to contain the list of the available representations without fixing
/// its shape; Andromeda answers an object holding only `availableRepresentations`, the media types the endpoint
/// renders, in the order of clause 6.3.4
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9005_an_entity_refusing_xml_lists_all_three_representations_as_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "not-acceptable")?;
    let request = accept(
        broker.unchecked(&RetrieveEntity::builder().id(&building_id).format(Lossless::Normalized).build())?,
        "application/xml",
    );
    let answer = broker.send_unchecked(request).await;
    let (head, body) = not_acceptable(&answer).or_fail()?;
    expect_that!(header_value(head.headers(), &CONTENT_TYPE), some(eq("application/json")));
    expect_that!(body, json_count("$", 1));
    expect_that!(body, json_count("$.availableRepresentations", 3));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    expect_that!(body, json_equals("$.availableRepresentations[2]", "application/geo+json"));
    Ok(())
}
