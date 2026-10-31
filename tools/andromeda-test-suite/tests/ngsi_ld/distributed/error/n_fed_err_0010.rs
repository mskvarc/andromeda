//! An unreachable retrieve source adds warning 199.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::{Entity, Relationship},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity of a locally held Entity whose matching registration names an origin where nothing listens
/// answers 200 with the local Entity and NGSILD-Warning code 199
/// Clause: 6.3.17, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_err_0010_an_unreachable_retrieve_source_adds_warning_199() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
