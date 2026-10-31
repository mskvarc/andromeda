//! Create reports the registration whose source cannot be reached beside a peer success.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole, SuiteConfig},
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    partial_outcome::PartialSuccess,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Batch Entity Creation on broker A of one Entity routed by `idPattern` to a redirect registration of broker
/// B and one routed to a redirect registration of an origin where nothing listens answers 207 with the first as
/// success and the second as the one error naming its registration, and broker B holds the first
/// Clause: 5.6.7.4, 5.2.16, 5.2.17
#[gtest]
#[tokio::test]
async fn n_fed_bat_8001_create_reports_the_registration_whose_source_cannot_be_reached_beside_a_peer_success() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_b_id = namespace.registration_id("b")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_x_id = namespace.registration_id("x")?;
    let building_mad_1_id = namespace.urn("Building", "mad-1")?;
    let building_fail_1_id = namespace.urn("Building", "fail-1")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending("Building", "mad-.*")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            SupportedOperation::from(OperationGroup::RedirectionOps),
            SupportedOperation::from(DistributedOperation::CreateBatch),
        ])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_b_cleanup = Cleanup::of(&broker).registration(&registration_b_id);
    let registration_x = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_x_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending("Building", "fail-.*")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            SupportedOperation::from(OperationGroup::RedirectionOps),
            SupportedOperation::from(DistributedOperation::CreateBatch),
        ])
        .build()?;
    broker.registrations().register(&registration_x).send().await?;
    let _registration_x_cleanup = Cleanup::of(&broker).registration(&registration_x_id);
    let entities = [
        Entity::builder()
            .id(building_mad_1_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(building_fail_1_id.clone())
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    let report = broker.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_mad_1_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_fail_1_id).registration(&registration_x_id))
    );
    let retrieved = broker_b.with_accept(Accept::any()).entities().retrieve(&building_mad_1_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
