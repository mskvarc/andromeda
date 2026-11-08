//! Each discovery dimension returns the registrations the documents admit.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityTypeName, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, OperationGroup, RegistrationInfo, SupportedOperation},
};

/// Pins: Query Context Source Registrations by type, by type and `idPattern`, and by attrs naming a property or a
/// relationship answer 200 with the registration declaring it and without a registration declaring another type and
/// attribute
/// Clause: 5.10.2, 5.10.2.4, 5.12
/// Tenant: private (a discovery by attrs alone matches every registration of the tenant whose information names no
/// attribute (clause 5.12), so in the shared tenant the registrations of other files fill the page and hide the
/// case's own; clause 4.14 keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn n_csr_qry_0015_each_discovery_dimension_returns_the_registrations_the_documents_admit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let registration_id = namespace.registration_id(1)?;
    let device_id = namespace.urn("Device", 1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?).with_id(device_id)])
            .property_names([format!("https://test.andromeda/{namespace}/temperature").as_str()])
            .relationship_names([format!("https://test.andromeda/{namespace}/installedBy").as_str()])
            .build()?])
        .operations([
            SupportedOperation::from(DistributedOperation::RetrieveEntity),
            SupportedOperation::from(OperationGroup::FederationOps),
        ])
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([format!("https://test.andromeda/{namespace}/Vehicle")])
            .property_names([format!("https://test.andromeda/{namespace}/speed").as_str()])
            .build()?])
        .build()?;
    tenant.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&tenant).registration(&registration_2_id);
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .id_pattern(namespace.id_pattern("Device")?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("https://test.andromeda/{namespace}/temperature").parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&registration_id)));
    let page_4 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("https://test.andromeda/{namespace}/installedBy").parse()?])
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&registration_id)));
    tenant.registrations().delete(&registration_id).send().await?;
    tenant.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
