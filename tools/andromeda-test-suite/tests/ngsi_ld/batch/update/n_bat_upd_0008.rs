//! An item for no existing entity is reported per entity.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::reported, report_matcher::batch_error};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, operation::BatchUpdate},
    model::EntityFragment,
};

/// Pins: a Batch Entity Update whose only item has no type and names an entity that does not exist answers 207 with
/// an error for that entity and nothing in success, not a request-level error
/// Clause: 5.6.9.4, 5.6.9.5, 6.16.3.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_0008_an_item_for_no_existing_entity_is_reported_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unknown_id = namespace.urn("Building", "unknown")?;
    let building_type = namespace.entity_type("Building")?;
    let fragments = [EntityFragment::for_entity(building_unknown_id.clone(), building_type).build()?];
    // The item names no type and carries a member no entity has, which the model cannot write, so the document is written as text.
    let document = UncheckedDocument::empty()
        .set_text("id", &building_unknown_id.to_string())
        .set_text("invalidField", "not a valid NGSI-LD structure");
    let request = broker.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?.documents(&[document]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, contains(batch_error().entity(&building_unknown_id)));
    for error in &report.errors {
        expect_that!(&error.error.error_type, some(anything()));
    }
    Ok(())
}
