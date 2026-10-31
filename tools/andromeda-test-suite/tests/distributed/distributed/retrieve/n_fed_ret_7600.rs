//! A retrieve reaches only the registrations holding retrieve entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity on broker A under inclusive registrations of one Entity advertising `retrieveEntity`,
/// `queryEntity`, both, `queryBatch`, and `retrieveEntity` at broker B, answers 200 merging the sources that hold
/// `retrieveEntity`, each contacted once, and contacts neither the `queryEntity-only` nor the `queryBatch-only`
/// source
/// Clause: 5.7.1.4, 4.20, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_ret_7600_a_retrieve_reaches_only_the_registrations_holding_retrieve_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_ids = (1..=5).map(|index| namespace.registration_id(index)).collect::<Result<Vec<_>, _>>()?;
    let vehicle_b = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker_b.entities().create(&vehicle_b).send().await?.done()?;
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    for prefix in ["s1", "s3"] {
        source
            .answer(
                Method::GET,
                &format!("/{prefix}/ngsi-ld/v1/entities/{vehicle_id}"),
                source_answer::entity(&source_entity),
            )
            .await?;
    }
    let source_entities = [];
    source
        .answer(Method::GET, "/s2/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    source
        .answer(Method::POST, "/s4/ngsi-ld/v1/entityOperations/query", source_answer::entities(&source_entities))
        .await?;
    let sources = [
        ("s1", vec![DistributedOperation::RetrieveEntity]),
        ("s2", vec![DistributedOperation::QueryEntity]),
        ("s3", vec![DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity]),
        ("s4", vec![DistributedOperation::QueryBatch]),
    ];
    for ((prefix, operations), registration_id) in sources.into_iter().zip(registration_ids.iter()) {
        let registration = CSourceRegistration::builder(source.endpoint(prefix)?)
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
                .build()?])
            .mode(RegistrationMode::Inclusive)
            .operations(operations)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    let registration_5 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_ids.get(4).cloned().or_fail()?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["brandName"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_5).send().await?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(10)));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    for prefix in ["s1", "s3"] {
        let forwarded = source
            .received_matching(&SourceRequest::matching(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entities/{vehicle_id}")))
            .await?;
        expect_that!(forwarded, len(eq(1)));
    }
    for prefix in ["s2", "s4"] {
        let forwarded = source
            .received_matching(&SourceRequest::any_matching(&format!(
                "/{prefix}/ngsi-ld/v1/(entities|entityOperations|temporal).*"
            )))
            .await?;
        expect_that!(forwarded, len(eq(0)));
    }
    Ok(())
}
