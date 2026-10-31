//! The advertised list follows the refusing endpoint.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::{QueryEntities, RetrieveEntityTypes},
    model::Lossless,
    query::builders::SelectorTypeBuilder,
};

/// Pins: Query Entities accepting application/xml lists application/json, application/ld+json and
/// application/geo+json, and Retrieve Available Entity Types accepting application/xml lists only application/json
/// and application/ld+json
/// Clause: 6.3.4, 6.3.15
/// Recorded: clause 6.3.4 requires the 406 body to contain the list of the available representations without fixing
/// its shape; Andromeda answers an object holding only `availableRepresentations`, the media types the endpoint
/// renders, in the order of clause 6.3.4
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9007_the_advertised_list_follows_the_refusing_endpoint() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let selection = SelectorTypeBuilder::from(&namespace.entity_type("Building")?).build()?;
    let request = accept(
        broker.unchecked(&QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build())?,
        "application/xml",
    );
    let answer = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&answer).or_fail()?;
    expect_that!(body, json_count("$", 1));
    expect_that!(body, json_count("$.availableRepresentations", 3));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    expect_that!(body, json_equals("$.availableRepresentations[2]", "application/geo+json"));
    let request_2 = accept(broker.unchecked(&RetrieveEntityTypes::builder().build())?, "application/xml");
    let answer_2 = broker.send_unchecked(request_2).await;
    let (_, body_2) = not_acceptable(&answer_2).or_fail()?;
    expect_that!(body_2, json_count("$", 1));
    expect_that!(body_2, json_count("$.availableRepresentations", 2));
    expect_that!(body_2, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body_2, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
