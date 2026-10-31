//! Query temporal entities accepting geojson is 406 listing json and json ld.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    not_acceptable::not_acceptable,
    wire::accept,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::QueryTemporalEvolution,
    model::Lossless,
    query::{builders::SelectorTypeBuilder, temporal_query::TemporalQuery},
};

/// Pins: Query Temporal Evolution of Entities with Accept application/geo+json answers 406 listing the available
/// representations application/json, application/ld+json
/// Clause: 6.3.4, 6.3.15, 5.7.4
/// Recorded: 6.3.4 mandates the 406 and a body containing "the list of the available representations of the
/// resources" without fixing its shape; Andromeda answers `availableRepresentations`, the media types the endpoint
/// renders, in this order
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_0012_query_temporal_entities_accepting_geojson_is_406_listing_json_and_json_ld() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let selection = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let after = TemporalQuery::after("2020-08-01T12:00:00Z".parse()?);
    let request = accept(
        broker.unchecked(
            &QueryTemporalEvolution::builder()
                .entity_type(&selection)
                .temporal_q(&after)
                .format(Lossless::Normalized)
                .build(),
        )?,
        "application/geo+json",
    );
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_count("$.availableRepresentations", 2));
    expect_that!(body, json_equals("$.availableRepresentations[0]", "application/json"));
    expect_that!(body, json_equals("$.availableRepresentations[1]", "application/ld+json"));
    Ok(())
}
