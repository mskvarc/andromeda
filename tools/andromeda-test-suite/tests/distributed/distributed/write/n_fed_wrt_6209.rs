//! Exclusive merge forwards the owned attribute and merges the rest locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Merge Entity of speed and `brandName` under an exclusive registration of speed answers 204, forwards one
/// merge carrying speed alone, merges `brandName` locally and leaves the local speed untouched
/// Clause: 5.6.17.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6209_exclusive_merge_forwards_the_owned_attribute_and_merges_the_rest_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::MergeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let fragment = EntityFragment::builder().property("speed", 99).property("brandName", "BMW").build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let forwarded = source.received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::fragment(forwarded_one)?;
    expect_that!(&forwarded_entity, lacks_attribute("brandName"));
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(99)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("BMW")));
    expect_that!(entity, has_attribute("speed", any_attribute().value(42)));
    Ok(())
}
