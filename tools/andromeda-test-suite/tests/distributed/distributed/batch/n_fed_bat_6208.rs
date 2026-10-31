//! Upsert forwards each redirect source only the attributes it registers.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::{json_absent, json_equals},
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
    http::{UncheckedDocument, operation::BatchUpsert},
    model::{Entity, Lossless, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert of a Vehicle with `brandName`, speed and location under two redirect registrations of
/// speed and of location forwards each source one batch holding the Vehicle with its own attribute alone
/// Clause: 5.6.8.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_bat_6208_upsert_forwards_each_redirect_source_only_the_attributes_it_registers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::UpsertBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["location"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::UpsertBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/upsert",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/upsert",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?];
    // The model requires `location` to be a GeoProperty, so the Vehicle is written with it as text.
    let location: RawJson = r#"{"type": "Property", "value": "home"}"#.parse()?;
    let vehicle_document = entities
        .first()
        .map(|vehicle| UncheckedDocument::from_entity(vehicle, Lossless::Normalized).set_member("location", &location))
        .or_fail()?;
    let request = broker
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[vehicle_document]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/upsert").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        forwarded_body::entities(forwarded_one)?,
        nth(
            0,
            all!(
                has_id(&vehicle_id),
                has_attribute("speed", any_attribute().value(42)),
                lacks_attribute("location"),
                lacks_attribute("brandName")
            )
        )
    );
    let forwarded_2 = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/upsert").await?;
    expect_that!(forwarded_2, len(eq(1)));
    // The batch the second source receives holds `location` as a Property, which the entity reader refuses, so it is
    // read as received.
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(forwarded_one_2, json_equals("$[0].id", vehicle_id.to_string()));
    expect_that!(forwarded_one_2, json_equals("$[0].location.value", "home"));
    expect_that!(forwarded_one_2, json_absent("$[0].speed"));
    Ok(())
}
