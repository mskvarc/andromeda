//! A batch entity naming an attribute its @context expands to no IRI is a per-entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Creation refuses the entity whose attribute is named ex:a{b under an @context defining the
/// prefix ex with `BadRequestData` in the 207 answer and creates nothing of it, while the entity beside it naming
/// `ex:good_name` is created
/// Clause: 4.6.2, 5.6.7
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_bat_cre_9010_an_attribute_name_that_expands_to_no_iri_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_1_id = namespace.urn("Vehicle", "1")?;
    let vehicle_2_id = namespace.urn("Vehicle", "2")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [
        Entity::builder()
            .id(vehicle_1_id.clone())
            .entity_type(vehicle_type.clone())
            .property("placeholder", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type)
            .property("ex:good_name", 2)
            .build()?,
    ];
    // The model refuses a name that expands to no IRI, so the first entity's attribute is renamed to it.
    let broken = UncheckedDocument::from_entity(&entities[0], Lossless::Normalized).rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[broken, UncheckedDocument::from_entity(&entities[1], Lossless::Normalized)]);
    let report: BatchOperationResult = reported(writer.send_unchecked(request).await)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&vehicle_1_id).problem(ErrorType::BadRequestData)));
    expect_that!(&report.success, nth(0, eq(&vehicle_2_id)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_1_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("http://example.org/good_name", property().value(2)));
    Ok(())
}
