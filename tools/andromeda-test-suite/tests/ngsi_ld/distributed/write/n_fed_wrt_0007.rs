//! A replace under an exclusive registration forwards and keeps the registered attribute out locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Relationship},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Entity with speed and `fuelType` of an Entity held locally whose speed is registered exclusively
/// answers 204, forwards one replace, and the local Entity holds `fuelType`, no longer `brandName`, and no speed
/// Clause: 5.6.18.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0007_a_replace_under_an_exclusive_registration_forwards_and_keeps_the_registered_attribute_out_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([
            DistributedOperation::CreateEntity,
            DistributedOperation::UpdateEntity,
            DistributedOperation::DeleteEntity,
            DistributedOperation::MergeEntity,
            DistributedOperation::ReplaceEntity,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(
            SourceRequest::matching(Method::PUT, "/a/ngsi-ld/v1/entities/[^/]+"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 120)
        .property("fuelType", "diesel")
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let forwarded_request = SourceRequest::matching(Method::PUT, "/a/ngsi-ld/v1/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, lacks_attribute("brandName"));
    expect_that!(entity, has_attribute("fuelType", any_attribute().value("diesel")));
    Ok(())
}
