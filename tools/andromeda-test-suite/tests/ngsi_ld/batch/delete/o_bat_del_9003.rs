//! A batch delete still reports an unknown identifier.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, partial_outcome::PartialSuccess, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Batch Entity Delete of an Entity whose Temporal Evolution was deleted and of an unknown identifier answers
/// 207 with a `ResourceNotFound` error for the unknown one only
/// Clause: 5.2.16, 5.6.10.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_bat_del_9003_a_batch_delete_still_reports_an_unknown_identifier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_present_id = namespace.urn("Building", "batch-present")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_absent_id = namespace.urn("Building", "batch-absent")?;
    let building_batch_present = Entity::builder()
        .id(building_batch_present_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_batch_present).send().await?.done()?;
    broker.temporal().delete(&building_batch_present_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_batch_present_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let ids = [building_batch_present_id.clone(), building_batch_absent_id.clone()];
    let report = broker.entities().batch_delete(&ids).send().await?.partial()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_batch_absent_id).problem(ErrorType::ResourceNotFound))
    );
    expect_that!(&report.errors, not(contains(batch_error().entity(&building_batch_present_id))));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_present_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
