//! A failing type listing source adds its warning.

use andromeda_test_suite::{broker, config::SuiteConfig, context_source::ContextSource, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, NgsiLdWarnings, WarningCode},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Entity Types whose only registered source answers 500 answers 200 with NGSILD-Warning code
/// 299; once that registration is gone and the only one names an origin where nothing listens, with code 199
/// Clause: 6.3.17, 5.7.5.4
/// Tenant: private (an unfiltered /types or /attributes listing is matched by every registration of the tenant, clause
/// 5.12, and pages over every file's entries)
/// Recorded: 5.7.11 makes forwarding a discovery request optional and 6.3.17 mandates NGSILD-Warning only for /entities
/// and `/entities/{entityId}`; Andromeda forwards discovery and warns of a failing source
#[gtest]
#[tokio::test]
async fn o_fed_dsc_0002_a_failing_type_listing_source_adds_its_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_id_2 = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/types", source_answer::status(StatusCode::INTERNAL_SERVER_ERROR))
        .await?;
    let listed = broker.with_heads().with_accept(Accept::any()).entity_types().list().send().await?;
    expect_that!(
        listed
            .head
            .typed::<NgsiLdWarnings>()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    broker.registrations().delete(&registration_id).send().await?;
    let unreachable = CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        .id(registration_id_2)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&unreachable).send().await?;
    let listed_2 = broker.with_heads().with_accept(Accept::any()).entity_types().list().send().await?;
    expect_that!(
        listed_2
            .head
            .typed::<NgsiLdWarnings>()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    Ok(())
}
