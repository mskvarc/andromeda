//! A batch upsert item naming an attribute its @context expands to no IRI is a per-entity bad request data.

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
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert under an @context defining the prefix ex refuses the entity whose attribute is named
/// ex:a{b with `BadRequestData` in the 207 answer and creates nothing of it, while the entity beside it is created
/// Clause: 4.6.2, 5.6.8.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_bat_ups_9024_an_upsert_item_naming_an_attribute_that_expands_to_no_iri_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let refused_id = namespace.urn("Building", "refused")?;
    let upserted_id = namespace.urn("Building", "upserted")?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(refused_id.clone())
            .entity_type(building_type.clone())
            .property("placeholder", 1)
            .build()?,
        Entity::builder()
            .id(upserted_id.clone())
            .entity_type(building_type)
            .property("temperature", 21)
            .build()?,
    ];
    // The model refuses a name that expands to no IRI, so the first entity's attribute is renamed to it.
    let broken = UncheckedDocument::from_entity(&entities[0], Lossless::Normalized).rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[broken, UncheckedDocument::from_entity(&entities[1], Lossless::Normalized)]);
    let report: BatchOperationResult = reported(writer.send_unchecked(request).await)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&refused_id).problem(ErrorType::BadRequestData)));
    expect_that!(&report.success, nth(0, eq(&upserted_id)));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let answer = json.with_responses().entities().retrieve(&refused_id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let retrieved = json.entities().retrieve(&upserted_id).send().await?;
    expect_that!(retrieved.body(), has_attribute("temperature", property().value(21)));
    Ok(())
}
