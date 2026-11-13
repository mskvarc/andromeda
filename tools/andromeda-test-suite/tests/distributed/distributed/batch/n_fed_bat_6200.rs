//! Create forwards each redirect source only the attributes it registers.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::BatchCreate},
    model::{
        Entity,
        geometry::{Position, Shape},
    },
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation of a Vehicle with `brandName`, speed and location plus a Device, under two redirect
/// registrations of speed and of location, forwards each source one batch holding the Vehicle with its own
/// attribute alone and never the Device, and creates `brandName` and the Device locally
/// Clause: 5.6.7.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_bat_6200_create_forwards_each_redirect_source_only_the_attributes_it_registers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["location"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", "Mercedes")
            .property("speed", 42)
            .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
            .build()?,
        Entity::builder()
            .id(device_id.clone())
            .entity_type(device_type)
            .property("brandName", "sensor")
            .build()?,
    ];
    let request = broker.unchecked(&BatchCreate::builder().entities(&entities).build())?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        forwarded_body::entities(forwarded_one)?,
        all!(
            len(eq(1)),
            nth(
                0,
                all!(
                    has_id(&vehicle_id),
                    has_attribute("speed", any_attribute().value(42)),
                    lacks_attribute("location"),
                    lacks_attribute("brandName")
                )
            )
        )
    );
    let forwarded_2 = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(
        forwarded_body::entities(forwarded_one_2)?,
        all!(
            len(eq(1)),
            nth(
                0,
                all!(
                    has_id(&vehicle_id),
                    has_attribute("location", any_attribute()),
                    lacks_attribute("speed"),
                    lacks_attribute("brandName")
                )
            )
        )
    );
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, lacks_attribute("location"));
    broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&device_id)
        .locality(Locality::Local)
        .send()
        .await?;
    Ok(())
}
