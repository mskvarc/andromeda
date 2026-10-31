//! Delete reports an over long id and deletes the rest.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Delete of an existing entity and an id past the identifier bound answers 207, deletes the
/// entity, and reports the other id naming the id and the bound
/// Clause: 5.6.10.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_bat_res_9007_delete_reports_an_over_long_id_and_deletes_the_rest() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_existing_id = namespace.urn("BatchEntityError", "existing")?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let batch_entity_error_delete_aaaaaaaaaaaaaaaaaaa_id = namespace.urn("BatchEntityError", format!("delete-{}", "a".repeat(987)).as_str())?;
    let entities = [Entity::builder()
        .id(batch_entity_error_existing_id.clone())
        .entity_type(batch_entity_error_type)
        .property("speed", 1)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The identifier is past the 1024-byte bound by however long the run makes {{ns}}, so its length is not asserted.
    let ids = [batch_entity_error_existing_id.clone(), batch_entity_error_delete_aaaaaaaaaaaaaaaaaaa_id.clone()];
    let report = broker.entities().batch_delete(&ids).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&batch_entity_error_existing_id)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&batch_entity_error_delete_aaaaaaaaaaaaaaaaaaa_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&batch_entity_error_existing_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
