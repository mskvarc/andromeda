//! A Temporal Evolution naming an attribute its @context expands to no IRI is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution whose attribute is named ex:a{b under an @context defining the prefix ex is refused with
/// 400 `BadRequestData` and the entity is not created
/// Clause: 4.6.2, 5.6.11.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9031_an_attribute_name_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("placeholder", Property::builder(1).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    // The model refuses a name that expands to no IRI, so the attribute is renamed to it.
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build())?
        .document(&document);
    expect_that!(
        &writer.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
