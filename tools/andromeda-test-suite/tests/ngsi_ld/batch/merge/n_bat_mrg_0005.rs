//! An item for no existing entity is reported per entity.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_matches},
    broker,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, operation::BatchMerge},
    model::EntityFragment,
};

/// Pins: a Batch Entity Merge whose only item has no type and names an entity that does not exist answers 207 with
/// an error for that entity and nothing in success, not a request-level error
/// Clause: 5.6.20.4, 5.6.20.5
#[gtest]
#[tokio::test]
async fn n_bat_mrg_0005_an_item_for_no_existing_entity_is_reported_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unknown_id = namespace.urn("Building", "unknown")?;
    let placeholder = [EntityFragment::for_entity(building_unknown_id.clone(), namespace.entity_type("Building")?).build()?];
    let item = UncheckedDocument::empty()
        .set_text("id", &building_unknown_id.to_string())
        .set_text("invalidField", "not a valid NGSI-LD structure");
    // The item has no `type` and a member that is no attribute, which no fragment holds, so it replaces the placeholder.
    let request = broker.unchecked(&BatchMerge::builder().fragments(&placeholder).build())?.documents(&[item]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::MULTI_STATUS));
    let received = answer?;
    let body = received.body();
    // The report model reads a missing `success` or `errors` as empty, so their presence is read from the body.
    expect_that!(body, json_count("$.success", 0));
    expect_that!(body, json_count("$.errors", 1));
    expect_that!(body, json_equals("$.errors[0].entityId", building_unknown_id.to_string()));
    expect_that!(body, json_matches("$.errors[0].error.type", ".+")?);
    let report: BatchOperationResult = serde_json::from_slice(body)?;
    expect_that!(&report.success, is_empty());
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&building_unknown_id)));
    Ok(())
}
