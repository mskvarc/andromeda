//! An anonymous registration gets an ngsi ld urn retrievable under it.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration without an id answers 201 with a Location naming an assigned
/// urn:ngsi-ld: identifier, and Retrieve Context Source Registration of that identifier answers it as its id
/// Clause: 5.9.2.4, 6.9.3.1, Table 5.2.9-1
#[gtest]
#[tokio::test]
async fn n_csr_cre_9001_an_anonymous_registration_gets_an_ngsi_ld_urn_retrievable_under_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    let created = broker.with_responses().registrations().register(&registration).send().await?;
    expect_that!(
        created
            .head
            .typed::<Location>()
            .valid()
            .map(|location| location.as_iri_ref().as_str().to_owned()),
        some(matches_regex("/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:[^/]+$"))
    );
    let registration_id = created.outcome.id().or_fail()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
