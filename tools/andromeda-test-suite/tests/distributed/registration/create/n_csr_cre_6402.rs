//! A redirect registration may name an id an id pattern a type or attributes only.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Context Source Registration with mode redirect by Entity id, by `idPattern`, by type only and by
/// attribute names only answers 201 for each and each reads back with mode redirect
/// Clause: 4.3.6.3, 5.9.2.4
/// Tenant: private (a registration by type only or by attribute names only matches every id-only request of its
/// tenant (clause 5.12) and would hijack the requests of other files in the shared tenant)
#[gtest]
#[tokio::test]
async fn n_csr_cre_6402_a_redirect_registration_may_name_an_id_an_id_pattern_a_type_or_attributes_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    tenant.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&tenant).registration(&registration_2_id);
    let registration_3 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    tenant.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&tenant).registration(&registration_3_id);
    let registration_4 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .build()?;
    tenant.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&tenant).registration(&registration_4_id);
    let registration_5 = tenant.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_5.mode(), some(eq(RegistrationMode::Redirect)));
    let registration_6 = tenant.with_accept(Accept::any()).registrations().retrieve(&registration_2_id).send().await?;
    expect_that!(registration_6.mode(), some(eq(RegistrationMode::Redirect)));
    let registration_7 = tenant.with_accept(Accept::any()).registrations().retrieve(&registration_3_id).send().await?;
    expect_that!(registration_7.mode(), some(eq(RegistrationMode::Redirect)));
    let registration_8 = tenant.with_accept(Accept::any()).registrations().retrieve(&registration_4_id).send().await?;
    expect_that!(registration_8.mode(), some(eq(RegistrationMode::Redirect)));
    Ok(())
}
