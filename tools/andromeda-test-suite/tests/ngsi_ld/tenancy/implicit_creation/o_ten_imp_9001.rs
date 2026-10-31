//! A create entity refused as bad request data creates no tenant.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Create Entity on a new Tenant refused with 400 `BadRequestData` for an `observedAt` with seven fraction
/// digits leaves the Tenant nonexistent: a following query there answers 404 `NonexistentTenant` echoing the tenant
/// Clause: 5.5.10, 5.6.1, 5.5.4, 6.3.14, Table 6.3.2-1
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9001_a_create_entity_refused_as_bad_request_data_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let refused_tenant = broker.with_tenant(format!("{namespace}-refused").parse()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 55, "observedAt": "2024-01-01T00:00:00.1234567Z"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let request = refused_tenant.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = refused_tenant.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = refused_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(HeaderName::from_static("ngsild-tenant"), &format!("{namespace}-refused"))
    );
    Ok(())
}
