//! A registration naming a peer broker answers 201 at its id and reads back its context source info.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration on broker A with an explicit id, broker B as endpoint and a
/// `contextSourceInfo` accept pair answers 201 with a Location ending in that id, and the registration reads back
/// on broker A with the id and the pair
/// Clause: 5.9.2, 5.10.1, 5.2.9, 5.2.22
#[gtest]
#[tokio::test]
async fn n_csr_cre_7600_a_registration_naming_a_peer_broker_answers_201_at_its_id_and_reads_back_its_context_source_info() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let peer_b = broker::peer(PeerRole::B)?;
    let registration = CSourceRegistration::builder(peer_b.clone())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .context_source_info(("accept", "application/ld+json"))
        .build()?;
    let created = broker.with_responses().registrations().register(&registration).send().await?;
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(
        location,
        some(ends_with(format!(
            "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:{namespace}-1"
        )))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.endpoint(), eq(&peer_b));
    expect_that!(
        retrieved.context_source_info().and_then(|infos| infos.first()),
        some(eq(&ContextSourceInfo::parse("accept", "application/ld+json")?))
    );
    Ok(())
}
