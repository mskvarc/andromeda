//! An entity info with two types reads back with both.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, spelling_matcher::listing_at};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::OneOrMany,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose `EntityInfo` type is an array of two types answers 201, and the
/// registration reads back with both types in order
/// Clause: 5.9.2, 5.2.8
#[gtest]
#[tokio::test]
async fn n_csr_cre_6012_an_entity_info_with_two_types_reads_back_with_both() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(OneOrMany::try_from(vec![namespace.entity_type("Vehicle")?, namespace.entity_type("Car")?])?)
                    .with_id_pattern(namespace.id_pattern("Vehicle")?),
            ])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(listing_at(0, &vehicle_type))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(listing_at(1, &car_type))
    );
    Ok(())
}
