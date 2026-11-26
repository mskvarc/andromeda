//! A prefixed `objectType` holding a hyphen after the colon is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{HYPHENATED_TYPE, under_example_prefix},
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Attribute, Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Relationship, VocabTerm},
};

/// Pins: Create Entity whose @context defines the prefix ex and whose Relationship owner has the objectType
/// ex:bad-type answers 400 `BadRequestData` and creates nothing
/// Clause: 4.5.3.2, 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; an
/// objectType names entity types, so Andromeda holds it to the grammar an entity type is held to
#[gtest]
#[tokio::test]
async fn o_ent_cre_9059_a_prefixed_object_type_holding_a_hyphen_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_id = namespace.urn("Vehicle", "prefixed-object-type")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .build()?;
    let owner = Attribute::from(
        Relationship::builder(namespace.urn("Person", "o")?)
            .object_type("ex:Placeholder".parse::<VocabTerm>()?)
            .build()?,
    );
    // The model refuses an objectType outside the grammar of a name, so the member is set to it.
    let broken_owner: RawJson = UncheckedDocument::try_from(owner.render(Lossless::Normalized))?
        .set_text("objectType", HYPHENATED_TYPE)
        .to_string()
        .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("owner", &broken_owner);
    let request = writer.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    expect_that!(
        &writer.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
