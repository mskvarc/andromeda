//! An item leaving an axis unconstrained stays a candidate on it.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type, and by type with an `idPattern`, answer 200 including a
/// registration whose information names only a property, and by attrs alone answer 200 including a registration
/// whose information names only an entity type
/// Clause: 5.10.2, 5.10.2.4, 5.12
/// Tenant: private (a registration whose information names only a property matches every entity type (clause 5.12),
/// so in the shared tenant every query of another file naming no attributes would be forwarded to it; clause 4.14
/// keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn n_csr_qry_0014_an_item_leaving_an_axis_unconstrained_stays_a_candidate_on_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let device_type = namespace.entity_type("Device")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().property_names(["temperature"]).build()?])
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder().entities([device_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&tenant).registration(&registration_2_id);
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&device_type)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&device_type)
        .id_pattern(namespace.id_pattern("Device")?)
        .send()
        .await?;
    expect_that!(page_2.items(), contains(has_id(&registration_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), contains(has_id(&registration_2_id)));
    tenant.registrations().delete(&registration_id).send().await?;
    tenant.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
