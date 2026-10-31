//! An unreadable source entity is left out with warning 111.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity of a local Entity whose inclusive source answers a Property without its value answers 200
/// with the local data only and NGSILD-Warning code 111
/// Clause: 5.7.1.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_ret_7404_an_unreadable_source_entity_is_left_out_with_warning_111() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("fuelLevel", 70)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The model requires a Property to hold a value, so the source's entity is written as text.
    let unreadable = format!(r#"{{"id":"{vehicle_id}","type":"Vehicle_{namespace}","speed":{{"type":"Property"}}}}"#);
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &unreadable),
        )
        .await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().value(70)));
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().find(|warning| warning.code() == WarningCode::RevalidationFailed))
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::RevalidationFailed))
    );
    Ok(())
}
