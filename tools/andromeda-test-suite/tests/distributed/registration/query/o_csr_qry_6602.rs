//! A returned registration holds only the matching registration info.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type answers 200 with a registration of two `RegistrationInfo` items
/// holding only the one naming the type
/// Clause: 5.10.2.5
/// Recorded: clause 5.10.2.5 says implementations should return filtered registrations holding only the matching
/// `RegistrationInfo` elements, so trimming is optional
#[gtest]
#[tokio::test]
async fn o_csr_qry_6602_a_returned_registration_holds_only_the_matching_registration_info() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let other_type = namespace.entity_type("Other")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?,
            RegistrationInfo::builder()
                .entities([EntityInfo::new(other_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let returned = page.items().iter().find(|candidate| candidate.id() == &registration_id).or_fail()?;
    expect_that!(returned.information(), len(eq(1)));
    let entities = returned.information().first().or_fail()?.entities();
    expect_that!(entities.first().map(EntityInfo::entity_type), some(eq(&OneOrMany::from(vehicle_type))));
    expect_that!(returned.information().get(1), none());
    Ok(())
}
