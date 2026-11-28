//! Two holders of one instance answer the one modified later.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    context_source::ContextSource,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    peer_role::PeerRole,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution on broker A answers, of the speed instance broker B holds and a source answers
/// under the same `instanceId` and `observedAt`, only the one modified later: the source's speed 20 modified in 2100
/// over broker B's 10 for Vehicle 1, and broker B's 30 over the source's 40 modified in 2000 for Vehicle 2, written
/// without `modifiedAt` because the request did not ask for `sysAttrs`
/// Clause: 5.7.3.4, 4.5.5.3, 4.5.7, ETSI TS 104 175 V1.1.1 clause 8.5.3
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7466_two_holders_of_one_instance_answer_the_one_modified_later() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_evolution_b = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_b).send().await?.created()?;
    let vehicle_2_evolution_b = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_2_evolution_b).send().await?.created()?;
    let held_by_b = broker_b
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let instance_id = instance_id_at(held_by_b.body(), &"speed".parse()?, 0)?;
    let held_by_b_2 = broker_b
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_2_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let instance_2_id = instance_id_at(held_by_b_2.body(), &"speed".parse()?, 0)?;
    let source_evolution = TemporalEntity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .output_attribute(
            "speed",
            Property::output_builder(Property::builder(20).observed_at("2020-08-01T12:00:00Z".parse()?))
                .modified_at("2100-01-01T00:00:00Z".parse()?)
                .instance_id(instance_id),
        )
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::broker_temporal_entity(&source_evolution),
        )
        .await?;
    let source_evolution_2 = TemporalEntity::output_builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .output_attribute(
            "speed",
            Property::output_builder(Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?))
                .modified_at("2000-01-01T00:00:00Z".parse()?)
                .instance_id(instance_2_id),
        )
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s/ngsi-ld/v1/temporal/entities/{vehicle_2_id}"),
            source_answer::broker_temporal_entity(&source_evolution_2),
        )
        .await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20).without(Member::ModifiedAt))))
    );
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_2_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(30).without(Member::ModifiedAt))))
    );
    Ok(())
}
