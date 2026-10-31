//! An entity whose source attributes all expired is discarded.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type whose inclusive source answers one Entity with every attribute expired and one
/// valid Entity answers only the valid one
/// Clause: 5.7.2.4, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_fed_qry_7403_an_entity_whose_source_attributes_all_expired_is_discarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [
        Entity::output_builder()
            .id(vehicle_id)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(90).expires_at("2000-01-01T00:00:00Z".parse()?))
            .build()?,
        Entity::output_builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 50)
            .build()?,
    ];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_2_id)));
    Ok(())
}
