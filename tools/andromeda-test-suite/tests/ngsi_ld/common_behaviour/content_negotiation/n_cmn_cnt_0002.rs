//! Retrieve types accepting geojson is 406 naming the available representations.

use andromeda_test_suite::{body_json::json_includes, broker, not_acceptable::not_acceptable, wire::accept};
use googletest::prelude::*;
use ngsi_ld::http::operation::RetrieveEntityTypes;
use serde_json::json;

/// Pins: Retrieve Available Entity Types asked for application/geo+json answers 406, and the body names the
/// representations the resource has, application/json and application/ld+json
/// Clause: 6.3.4, 6.25.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_0002_retrieve_types_accepting_geojson_is_406_naming_the_available_representations() -> Result<()> {
    let broker = broker::front()?;
    let request = accept(broker.unchecked(&RetrieveEntityTypes::builder().build())?, "application/geo+json");
    let response = broker.send_unchecked(request).await;
    let (_, body) = not_acceptable(&response).or_fail()?;
    expect_that!(body, json_includes("$.availableRepresentations", json!("application/json")));
    expect_that!(body, json_includes("$.availableRepresentations", json!("application/ld+json")));
    Ok(())
}
