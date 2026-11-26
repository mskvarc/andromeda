//! A temporal Relationship instance with a prefixed `objectType` holding a hyphen after the colon is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{HYPHENATED_TYPE, under_example_prefix},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Attribute, ErrorType, Lossless, NgsiLdMediaType, RawJson, Relationship, VocabTerm, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution whose @context defines the prefix ex and whose Relationship owner holds an
/// instance with the objectType ex:bad-type answers 400 `BadRequestData` and creates nothing
/// Clause: 4.5.3.2, 4.6.2, 5.6.11.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; an
/// objectType names entity types, so Andromeda holds it to the grammar an entity type is held to
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9096_a_temporal_relationship_with_a_hyphenated_object_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .build()?;
    let owner = Attribute::from(
        Relationship::builder(namespace.urn("Person", "o")?)
            .object_type("ex:Placeholder".parse::<VocabTerm>()?)
            .observed_at("2020-08-01T12:03:00Z".parse()?)
            .build()?,
    );
    // The model refuses an objectType outside the grammar of a name, so the instance's member is set to it.
    let instance = UncheckedDocument::try_from(owner.render(Lossless::Normalized))?.set_text("objectType", HYPHENATED_TYPE);
    let instances: RawJson = format!("[{instance}]").parse()?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member("owner", &instances);
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
