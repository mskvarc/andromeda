//! Replaced information reads back in place of the old.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityTypeName, OneOrMany},
    query::IdPattern,
    registration::{
        CSourceRegistration,
        DistributedOperation,
        EntityInfo,
        OperationGroup,
        RegistrationFragment,
        RegistrationInfo,
        RegistrationTypeMember,
        SupportedOperation,
    },
};

/// Pins: Update Context Source Registration with a fragment replacing information by one item of another entity
/// type answers 204, the registration then reads back with only that item, and Delete Context Source Registration
/// answers 204
/// Clause: 5.9.3, 5.10.1, 5.9.4
#[gtest]
#[tokio::test]
async fn n_csr_upd_0006_replaced_information_reads_back_in_place_of_the_old() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let device_id = namespace.urn("Device", 1)?;
    let vehicle_type = format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?;
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
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?)
                    .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Vehicle:{namespace}-.*").parse::<IdPattern>()?),
            ])
            .property_names([format!("https://test.andromeda/{namespace}/speed").as_str()])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.information(), len(eq(1)));
    expect_that!(registration_2.information().first().map(|info| info.entities().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type)))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        none()
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/speed").as_str()))
    );
    expect_that!(
        registration_2.information().first().map(|info| info.relationship_names().is_empty()),
        some(eq(true))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
