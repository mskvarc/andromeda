//! A failed forwarded write carries the status and detail of the source in its detail.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity matching a single exclusive registration whose source answers 403 with the detail no access
/// answers 502 whose detail names the status 403 and the source detail
/// Clause: 6.3.17
/// Recorded: clause 6.3.17 asks only that the error of an unsafe method be as informative as possible and does not
/// say what the detail member carries
#[gtest]
#[tokio::test]
async fn o_fed_err_7205_a_failed_forwarded_write_carries_the_status_and_detail_of_the_source_in_its_detail() -> Result<()> {
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
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            "/a/ngsi-ld/v1/entities",
            source_answer::text(
                StatusCode::FORBIDDEN,
                HeaderValue::from_static("application/json"),
                "{\"detail\": \"no access\"}",
            ),
        )
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    let answer = broker.with_responses().entities().create(&vehicle).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_GATEWAY, "https://uri.etsi.org/ngsi-ld/errors/BadGateway".parse::<ErrorType>()?)
            .with_detail_containing("403")
            .with_detail_containing("no access")
    );
    Ok(())
}
