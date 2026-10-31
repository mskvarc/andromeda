//! Query registrations accepting xml is 406 listing json and json ld.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{http::operation::QueryRegistrations, query::type_selection};

/// Pins: Query Context Source Registrations with Accept application/xml answers 406 listing the available
/// representations application/json, application/ld+json
/// Clause: 6.3.4, 5.10.2
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0008_query_registrations_accepting_xml_is_406_listing_json_and_json_ld() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let selection = type_selection::entity_type(&building_type).build()?;
    let request = accept(
        broker.unchecked(&QueryRegistrations::builder().entity_type(&selection).build())?,
        "application/xml",
    );
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_count("$.availableRepresentations", 2));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
