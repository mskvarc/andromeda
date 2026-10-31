//! A context source filter reads each defined member of a registration.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q::Query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationManagementInfo, RegistrationMode},
};

/// Pins: Query Context Source Registrations by type with a csf over `registrationName`, mode, management, `expiresAt`
/// as a `DateTime`, operations or description answers 200 with the registration that satisfies it and without the one
/// that does not, and a csf over the delivery statistic `timesSent` matches neither
/// Clause: 4.3.6.3, 4.9, 5.2.9, 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9013_a_context_source_filter_reads_each_defined_member_of_a_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let origin = config.unreachable_origin();
    let vehicles_id = namespace.registration_id("vehicles")?;
    let buildings_id = namespace.registration_id("buildings")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // A registration of one type and no identifier is asked by every retrieval by identifier of the tenant, so one left behind
    // answers other tests' retrievals with a cooldown warning.
    let _registration_cleanup = Cleanup::of(&broker).registration(&vehicles_id).registration(&buildings_id);
    let vehicles = CSourceRegistration::builder(origin.as_str())
        .id(vehicles_id.clone())
        .registration_name("vehicles")
        .description("vehicle telemetry source")
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(namespace.urn("Vehicle", 1)?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .management(RegistrationManagementInfo::builder().timeout(3000).build()?)
        .expires_at("2099-12-31T23:59:59Z".parse()?)
        .build()?;
    broker.registrations().register(&vehicles).send().await?;
    let buildings = CSourceRegistration::builder(origin.as_str())
        .id(buildings_id.clone())
        .registration_name("buildings")
        .description("building inventory")
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .management(RegistrationManagementInfo::builder().timeout(9000).build()?)
        .expires_at("2098-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.registrations().register(&buildings).send().await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for filter in [
        r#"registrationName=="vehicles""#,
        r#"mode=="exclusive""#,
        "management[timeout]<5000",
        "expiresAt>2099-01-01T00:00:00Z",
        r#"operations=="queryEntity""#,
        r#"description~="telemetry""#,
    ] {
        let page = json
            .registrations()
            .query()
            .entity_type(&vehicle_type)
            .csf(filter.parse::<Query>()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(page.items(), nth(0, has_id(&vehicles_id)));
    }
    let page = json
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .csf("timesSent".parse::<Query>()?)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
