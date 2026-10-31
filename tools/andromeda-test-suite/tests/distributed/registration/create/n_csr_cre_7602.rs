//! A registration without an id gets a uri retrievable under its location.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration on broker A naming broker B, without an id, answers 201 with a Location
/// naming a URI, and Retrieve Context Source Registration of that URI answers 200 with it as id
/// Clause: 5.9.2, 5.10.1, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_7602_a_registration_without_an_id_gets_a_uri_retrievable_under_its_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    let created = broker.with_responses().registrations().register(&registration).send().await?;
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(
        location,
        some(contains_regex("/ngsi-ld/v1/csourceRegistrations/[A-Za-z][A-Za-z0-9+.-]*:[^/]+$"))
    );
    let generated_id = created.outcome.id().or_fail()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&generated_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&generated_id));
    broker.registrations().delete(&generated_id).send().await?;
    Ok(())
}
