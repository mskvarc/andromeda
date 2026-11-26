//! Added instances naming an attribute the request's @context expands to no IRI are bad request data and add nothing.

use andromeda_test_suite::{
    broker,
    evolution_matcher::has_evolution,
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AddTemporalAttributes},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution whose @context defines the prefix ex, adding a speed instance beside an
/// instance of an attribute named ex:a{b, answers 400 `BadRequestData` and the Vehicle keeps its one speed instance
/// Clause: 4.6.2, 5.6.12.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_tmp_add_9377_added_instances_naming_an_attribute_that_expands_to_no_iri_are_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let writer = under_example_prefix(&broker)?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("placeholder", Property::builder(1).observed_at("2020-09-01T12:05:00Z".parse()?))
        .build()?
        .attributes;
    // The model refuses a name that expands to no IRI, so the attribute is renamed to it.
    let document = UncheckedDocument::try_from(instances.render(Lossless::Normalized))?.rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&instances).build())?
        .document(&document);
    expect_that!(
        &writer.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let evolution = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(evolution.body(), has_evolution("speed", len(eq(1))));
    Ok(())
}
