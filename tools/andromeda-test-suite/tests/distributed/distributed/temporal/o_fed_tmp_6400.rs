//! A forwarded temporal write keeps every instance and its observed at.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{Property, RawJson, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create or Update Temporal Evolution of an Entity with three speed instances under an inclusive
/// registration declaring `ngsildConformance` 1.0 is forwarded once with all three instances, each with its
/// `observedAt`
/// Clause: 4.3.6.8, 5.6.11.4
/// Recorded: Tables 4.3.6.8-1 to 4.3.6.8-3 describe Entity data and do not say how a temporal representation is
/// amended; Andromeda keeps the instances and their `observedAt`
#[gtest]
#[tokio::test]
async fn o_fed_tmp_6400_a_forwarded_temporal_write_keeps_every_instance_and_its_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpsertTemporal])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "ngsildConformance", "value": "1.0"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/ngsi-ld/v1/temporal/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2025-01-02T00:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2025-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let forwarded_request = SourceRequest::new(Method::POST, "/ngsi-ld/v1/temporal/entities").body(BodyPattern::Selects("$.speed[2]".parse()?));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/temporal/entities")
        .body(BodyPattern::Selects("$.speed[0].observedAt".parse()?))
        .body(BodyPattern::Selects("$.speed[1].observedAt".parse()?))
        .body(BodyPattern::Selects("$.speed[2].observedAt".parse()?));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
