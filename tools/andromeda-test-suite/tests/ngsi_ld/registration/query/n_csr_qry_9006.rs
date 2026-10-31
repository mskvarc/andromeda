//! A registration matching on no element is not returned.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by a type none of the two `RegistrationInfo` elements names answers 200
/// without the registration
/// Clause: 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_9006_a_registration_matching_on_no_element_is_not_returned() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_two_kinds_id = namespace.registration_id("two-kinds")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_type = namespace.entity_type("OffStreetParking")?;
    let registration_two_kinds = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_two_kinds_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
                .property_names(["brandName"])
                .build()?,
            RegistrationInfo::builder()
                .entities([EntityInfo::new(off_street_parking_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
                .property_names(["availableSpotNumber"])
                .build()?,
        ])
        .build()?;
    broker.registrations().register(&registration_two_kinds).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_two_kinds_cleanup = Cleanup::of(&broker).registration(&registration_two_kinds_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("NothingRegistered")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    broker.registrations().delete(&registration_two_kinds_id).send().await?;
    Ok(())
}
