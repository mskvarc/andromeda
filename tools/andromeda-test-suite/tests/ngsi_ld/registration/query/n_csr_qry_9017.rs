//! A discovery type takes the whole entity type selection.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    query::type_selection,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations over registrations of Home, of Vehicle, of Home and Vehicle together and
/// of Motorhome answers `Home|Vehicle` with the first three, `(Home;Vehicle)` with the one naming both types, and
/// `Motorhome|(Home;Vehicle)` with that one and the Motorhome registration
/// Clause: 4.17, 5.10.2.3, 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_9017_a_discovery_type_takes_the_whole_entity_type_selection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let home_type = namespace.entity_type("Home")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let motorhome_type = namespace.entity_type("Motorhome")?;
    let home_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.registration_id(2)?;
    let home_vehicle_id = namespace.registration_id(3)?;
    let motorhome_id = namespace.registration_id(4)?;
    let home = CSourceRegistration::builder(source.endpoint("1")?)
        .id(home_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(home_type.clone())]).build()?])
        .build()?;
    broker.registrations().register(&home).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _home_cleanup = Cleanup::of(&broker).registration(&home_id);
    let vehicle = CSourceRegistration::builder(source.endpoint("2")?)
        .id(vehicle_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .build()?;
    broker.registrations().register(&vehicle).send().await?;
    let _vehicle_cleanup = Cleanup::of(&broker).registration(&vehicle_id);
    let home_vehicle = CSourceRegistration::builder(source.endpoint("3")?)
        .id(home_vehicle_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(OneOrMany::try_from(vec![home_type.clone(), vehicle_type.clone()])?)])
            .build()?])
        .build()?;
    broker.registrations().register(&home_vehicle).send().await?;
    let _home_vehicle_cleanup = Cleanup::of(&broker).registration(&home_vehicle_id);
    let motorhome = CSourceRegistration::builder(source.endpoint("4")?)
        .id(motorhome_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(motorhome_type.clone())]).build()?])
        .build()?;
    broker.registrations().register(&motorhome).send().await?;
    let _motorhome_cleanup = Cleanup::of(&broker).registration(&motorhome_id);
    let either = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(type_selection::entity_type(&home_type).or(&vehicle_type))
        .send()
        .await?;
    expect_that!(either.items(), len(eq(3)));
    expect_that!(either.items(), contains(has_id(&home_id)));
    expect_that!(either.items(), contains(has_id(&vehicle_id)));
    expect_that!(either.items(), contains(has_id(&home_vehicle_id)));
    // 4.17: a conjunction selects "only those Entities that have the specified Entity Type(s), possibly among
    // others", so a registration of Home alone or of Vehicle alone does not satisfy it.
    let both = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(type_selection::all_of([&home_type, &vehicle_type]))
        .send()
        .await?;
    expect_that!(both.items(), len(eq(1)));
    expect_that!(both.items(), contains(has_id(&home_vehicle_id)));
    let mixed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(type_selection::entity_type(&motorhome_type).or_all_of([&home_type, &vehicle_type]))
        .send()
        .await?;
    expect_that!(mixed.items(), len(eq(2)));
    expect_that!(mixed.items(), contains(has_id(&home_vehicle_id)));
    expect_that!(mixed.items(), contains(has_id(&motorhome_id)));
    Ok(())
}
