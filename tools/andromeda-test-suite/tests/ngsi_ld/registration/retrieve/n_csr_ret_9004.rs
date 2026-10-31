//! A registration reads back every member it was written with.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    geometry_matcher::geometry,
    namespace::Namespace,
    problem::answered,
    rectangle_polygon::rectangle_polygon,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{DateTime, NgsiLdMediaType, OneOrMany, RawJson, Value},
    registration::{CSourceRegistration, ContextSourcePropertyName, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Retrieve Context Source Registration answers every member Create wrote: `registrationName`, description,
/// endpoint, mode, operations, information, location, `expiresAt` and an additional member
/// Clause: 5.2.2, Table 5.2.9-1, 5.9.2, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_ret_9004_a_registration_reads_back_every_member_it_was_written_with() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id("typed")?;
    let vehicle_id = namespace.urn("Vehicle", "typed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .registration_name("typed round trip")
        .description("every member the client may supply")
        .location(rectangle_polygon(100.0, 0.0, 101.0, 1.0)?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    // An additional member is not a property the model writes, so it is written as text.
    let note: RawJson = r#""an additional member the broker does not know""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("customNote", &note);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.name(), some(eq("typed round trip")));
    expect_that!(retrieved.description(), some(eq("every member the client may supply")));
    expect_that!(
        retrieved.endpoint().origin().ascii_serialization(),
        eq(&config.unreachable_origin().origin().ascii_serialization())
    );
    expect_that!(retrieved.mode(), some(eq(RegistrationMode::Exclusive)));
    expect_that!(
        retrieved.operations(),
        some(elements_are![eq(&SupportedOperation::from(DistributedOperation::RetrieveEntity))])
    );
    expect_that!(retrieved.information(), len(eq(1)));
    let info = retrieved.information().first().or_fail()?;
    expect_that!(info.entities(), len(eq(1)));
    expect_that!(info.entities().first().and_then(EntityInfo::id), some(eq(&vehicle_id)));
    expect_that!(
        info.entities().first().map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    expect_that!(
        info.property_names().iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("speed")]
    );
    expect_that!(
        retrieved.location(),
        some(
            geometry()
                .of_type("Polygon")
                .length([0], 5)
                .ordinate([0, 1, 0], Value::Number("101.0".parse()?))
                .ordinate([0, 2, 1], Value::Number("1.0".parse()?))
        )
    );
    expect_that!(retrieved.expires_at(), some(eq(&"2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(
        retrieved.properties().get(&"customNote".parse::<ContextSourcePropertyName>()?),
        some(eq(&Value::from("an additional member the broker does not know")))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
