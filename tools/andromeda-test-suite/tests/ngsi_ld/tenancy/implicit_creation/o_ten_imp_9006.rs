//! A refused context source registration creates no tenant.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Register Context Source on a new Tenant refused with 400 `BadRequestData` for an operation no group or
/// operation of clause 4.20 names leaves the Tenant nonexistent: querying registrations there answers 404
/// `NonexistentTenant` echoing the tenant
/// Clause: 5.5.10, 5.9.2, 4.20, 6.3.14, Table 6.3.2-1
/// Tenant: private (the subject is creating a Tenant)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option, created "when an NGSI-LD
/// operation for creating information targets a new Tenant"; Andromeda creates a Tenant only once a creating
/// operation has created information in it, so a refused one creates none
#[gtest]
#[tokio::test]
async fn o_ten_imp_9006_a_refused_context_source_registration_creates_no_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    broker::private_tenant(&broker, &namespace).await?;
    let refused_tenant = broker.with_tenant(format!("{namespace}-refused").parse()?);
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (`partialAttrUpdate` is not a valid value of `operations` at operations), so `operations` is written as text.
    let operations: RawJson = r#"["partialAttrUpdate"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("operations", &operations);
    let request = refused_tenant
        .unchecked(&RegisterContextSource { registration: &registration })?
        .document(&document);
    let answer = refused_tenant.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = refused_tenant
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
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
