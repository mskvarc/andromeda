//! A discovered registration keeps only its matching information.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by the type of one of two `RegistrationInfo` elements answers the
/// registration with that element alone
/// Clause: 5.10.2.4, 5.10.2.5
/// Recorded: 5.10.2.5 recommends ("should") returning only the `RegistrationInfo` elements relevant for the query;
/// Andromeda follows the recommendation
#[gtest]
#[tokio::test]
async fn o_csr_qry_9005_a_discovered_registration_keeps_only_its_matching_information() -> Result<()> {
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
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
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
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let listed = page.items().first().or_fail()?;
    expect_that!(listed.information(), len(eq(1)));
    expect_that!(listed.information().first().map(|info| info.entities().len()), some(eq(1)));
    expect_that!(
        listed
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    expect_that!(
        listed
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("brandName"))
    );
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_two_kinds_id)));
    broker.registrations().delete(&registration_two_kinds_id).send().await?;
    Ok(())
}
