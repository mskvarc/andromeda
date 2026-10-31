//! An invalid batch item whose id is a context alias is reported against that id.

use andromeda_test_suite::{
    broker,
    core_context::core_context,
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation of a valid item beside one writing its id under a term its own @context maps to @id
/// and whose type is a number answers 207 with the first in success and a `BadRequestData` error against the aliased
/// id, and creates nothing of the second
/// Clause: 4.4, 5.6.7.4
#[gtest]
#[tokio::test]
async fn n_bat_cre_9338_an_invalid_batch_item_whose_id_is_a_context_alias_is_reported_against_that_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let building_batch_valid_id = namespace.urn("Building", "batch-valid")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_eid_invalid_id = namespace.urn("Building", "batch-eid-invalid")?;
    let entities = [
        Entity::builder()
            .id(building_batch_valid_id.clone())
            .entity_type(building_type.clone())
            .build()?,
        Entity::builder().id(building_batch_eid_invalid_id.clone()).entity_type(building_type).build()?,
    ];
    // JSON-LD 1.1 section 4.1.3: a term the item's own `@context` maps to `@id` names the entity's id, so the
    // failed item is reported against it (table 5.2.17-1).
    let aliased: RawJson = r#"{"eid": "@id"}"#.parse()?;
    // The model refuses it (a type is a name), so `type` is written as text.
    let numeric_type: RawJson = "5".parse()?;
    let document = UncheckedDocument::from_entity(&entities[1], Lossless::Normalized)
        .rename_member("id", "eid")
        .set_member("type", &numeric_type)
        .set_member("@context", &aliased);
    let request = core_context_body
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[UncheckedDocument::from_entity(&entities[0], Lossless::Normalized), document]);
    let answer = core_context_body.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_batch_valid_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_batch_eid_invalid_id).problem(ErrorType::BadRequestData))
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_eid_invalid_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
