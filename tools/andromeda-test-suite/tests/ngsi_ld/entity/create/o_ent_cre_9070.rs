//! A prefixed `objectType` holding a hyphen after the colon is bad request data on a `ListRelationship` and on a
//! Relationship written as a sub-attribute.

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
    model::{Attribute, Entity, ErrorType, ListRelationship, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabTerm},
};

/// Pins: Create Entity whose @context defines the prefix ex, with a `ListRelationship` rooms whose objectType is
/// ex:bad-type, and with a Property speed holding a Relationship calibratedBy whose objectType is ex:bad-type, answers
/// 400 `BadRequestData` for each and creates nothing
/// Clause: 4.5.3.2, 4.5.22.2, 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; an
/// objectType names entity types, so Andromeda holds it to the grammar an entity type is held to wherever it is written
#[gtest]
#[tokio::test]
async fn o_ent_cre_9070_a_hyphenated_object_type_is_bad_request_data_on_a_list_relationship_and_a_sub_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let rooms = Attribute::from(
        ListRelationship::builder(vec![namespace.urn("Room", 1)?])
            .object_type(placeholder.clone())
            .build()?,
    );
    // The model refuses an objectType outside the grammar of a name, so the member is set to it.
    let broken_rooms: RawJson = UncheckedDocument::try_from(rooms.render(Lossless::Normalized))?
        .set_text("objectType", HYPHENATED_TYPE)
        .to_string()
        .parse()?;
    let calibrated_by = Attribute::from(Relationship::builder(namespace.urn("Device", 1)?).object_type(placeholder).build()?);
    let broken_calibrated_by: RawJson = UncheckedDocument::try_from(calibrated_by.render(Lossless::Normalized))?
        .set_text("objectType", HYPHENATED_TYPE)
        .to_string()
        .parse()?;
    let speed = Attribute::from(Property::builder(1).build()?);
    let broken_speed: RawJson = UncheckedDocument::try_from(speed.render(Lossless::Normalized))?
        .set_member("calibratedBy", &broken_calibrated_by)
        .to_string()
        .parse()?;
    for (position, (name, broken)) in [("rooms", &broken_rooms), ("speed", &broken_speed)].into_iter().enumerate() {
        let building_id = namespace.urn("Building", position)?;
        let building = Entity::builder()
            .id(building_id.clone())
            .entity_type(namespace.entity_type("Building")?)
            .build()?;
        let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member(name, broken);
        let request = writer.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
        expect_that!(
            &writer.send_unchecked(request).await,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
            "{name}"
        );
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&building_id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::NOT_FOUND), "{name}");
    }
    Ok(())
}
