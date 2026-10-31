//! Adding temporal attributes to an evolution held only by a registration is forwarded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    forwarded_body,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Add Attributes of speed to a Temporal Evolution the broker does not hold, matched by an exclusive
/// registration of speed advertising `appendAttrsTemporal`, answers 204 and forwards one add carrying speed
/// Clause: 5.6.12.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6214_adding_temporal_attributes_to_an_evolution_held_only_by_a_registration_is_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::AppendAttrsTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type("Placeholder")
        .property("speed", Property::builder(99).observed_at("2021-02-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_id, &instances).send().await?;
    let forwarded = source
        .received_on(Method::POST, &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_evolution = forwarded_body::evolution_fragment(forwarded_one)?;
    expect_that!(&forwarded_evolution, has_evolution("speed", nth(0, any_attribute().value(99))));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
