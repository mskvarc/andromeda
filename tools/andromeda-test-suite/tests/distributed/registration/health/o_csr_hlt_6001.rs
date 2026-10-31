//! Client supplied health members are not stored.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration carrying status failed, `timesSent` 99, `timesFailed` 42, `lastSuccess`
/// and `lastFailure` answers 201, and the registration that has forwarded nothing reads back with none of those
/// members
/// Clause: 5.2.9, 5.9.2
/// Recorded: Table 5.2.9-2 makes these members read-only and says implementations ignore them when provided;
/// whether a registration that has triggered no distributed operation reports them at all is left open, and
/// Andromeda reports none
#[gtest]
#[tokio::test]
async fn o_csr_hlt_6001_client_supplied_health_members_are_not_stored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model writes no read-only health member, so the client-supplied ones are written as text.
    let times_sent: RawJson = "99".parse()?;
    let times_failed: RawJson = "42".parse()?;
    let document = UncheckedDocument::try_from(registration.render())?
        .set_text("status", "failed")
        .set_member("timesSent", &times_sent)
        .set_member("timesFailed", &times_failed)
        .set_text("lastSuccess", "2000-01-01T00:00:00Z")
        .set_text("lastFailure", "2000-01-01T00:00:00Z");
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    let health = retrieved.output();
    expect_that!(health.status, none());
    expect_that!(health.times_sent, none());
    expect_that!(health.times_failed, none());
    expect_that!(health.last_success, none());
    expect_that!(health.last_failure, none());
    Ok(())
}
