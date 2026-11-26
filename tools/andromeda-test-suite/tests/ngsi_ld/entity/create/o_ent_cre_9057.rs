//! A sub-attribute name the request's @context expands to no IRI is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{UNEXPANDABLE_NAMES, under_example_prefix},
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Attribute, Entity, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Create Entity whose @context defines the prefix ex and whose Property speed holds a sub-attribute named
/// `ex:a{b`, `ex:{`, `ex:%`, `ex:"`, `urn:x:{` or `https://example.org/{` answers 400 `BadRequestData` for each and
/// creates nothing
/// Clause: 4.6.2, 5.6.1.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_ent_cre_9057_a_sub_attribute_name_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let speed = Attribute::from(Property::builder(1).property("placeholder", 2).build()?);
    for (position, name) in UNEXPANDABLE_NAMES.into_iter().enumerate() {
        let vehicle_id = namespace.urn("Vehicle", position)?;
        let vehicle = Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(namespace.entity_type("Vehicle")?)
            .build()?;
        // The model refuses a name that expands to no IRI, so the sub-attribute is renamed to it.
        let broken_speed: RawJson = UncheckedDocument::try_from(speed.render(Lossless::Normalized))?
            .rename_member("placeholder", name)
            .to_string()
            .parse()?;
        let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &broken_speed);
        let request = writer.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
        expect_that!(
            &writer.send_unchecked(request).await,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
            "{name}"
        );
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::NOT_FOUND), "{name}");
    }
    Ok(())
}
