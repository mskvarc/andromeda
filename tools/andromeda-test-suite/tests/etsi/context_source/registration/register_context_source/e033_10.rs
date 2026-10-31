//! A registration whose body is no registration at all, or whose expiration date is not a date, is refused with 400
//! (clause 5.9.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::{
    http::{UncheckedDocument, UncheckedRequest, operation::RegisterContextSource},
    model::RawJson,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};

/// Requires the answer to `request` to be 400 with no `Location`, as a request that creates nothing has no resource to
/// name (RFC 9110 clause 10.2.2).
async fn refused_without_location(broker: &BrokerClient, request: UncheckedRequest) -> Result<()> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::BAD_REQUEST))?;
    match answer {
        Err(error) => expect_that!(header_value(error.head().or_fail()?.headers(), &LOCATION), none()),
        Ok(_) => return fail!("the broker answered the refused registration with a response"),
    }
    Ok(())
}

/// Pins: Check that one cannot create a context source with invalid content
/// Clause: 5.9.2
/// Expected: the 400 answer carries no Location: a request that creates nothing has no resource for a Location to name
/// (RFC 9110 clause 10.2.2)
/// Tenant: private (a broker that failed to refuse the request would hold a Context Source Registration without
/// information, which matches every other file's queries in the shared tenant (clause 5.12) and would forward them;
/// clause 4.14 keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn e033_10_01_different_data_structure_than_csource_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant: BrokerClient = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &[]).await?;
    // No typed registration carries neither `information` nor `endpoint`; the rendered one has both members removed.
    let placeholder = CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .build()?;
    let document = UncheckedDocument::try_from(placeholder.render())?
        .remove_member("information")
        .remove_member("endpoint");
    let client = ld_json_body_client(&tenant, &vocabulary)?;
    let _registration = Cleanup::of(&client).registration_of(&placeholder);
    let request = client.unchecked(&RegisterContextSource { registration: &placeholder })?.document(&document);
    refused_without_location(&tenant, request).await
}

/// Pins: Check that one cannot create a context source with invalid content
/// Clause: 5.9.2
/// Expected: the 400 answer carries no Location: a request that creates nothing has no resource for a Location to name
/// (RFC 9110 clause 10.2.2)
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_10_02_create_a_context_source_registration_with_a_date_in_the_past() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = vehicle_parking_registration(
        &namespace,
        SuiteConfig::get()?.unreachable_origin(),
        namespace.urn("ContextSourceRegistration", 1)?,
        None,
    )?;
    // The case's expiration date carries a leading space, which no typed date holds.
    let past: RawJson = r#"" 2018-11-29T14:53:15Z""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("expiresAt", &past);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _registration = Cleanup::of(&client).registration_of(&registration);
    let request = client.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    refused_without_location(&broker, request).await
}
