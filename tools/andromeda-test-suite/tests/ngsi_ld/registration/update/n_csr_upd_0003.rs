//! An ngsi ld null expires at removes the expiration.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
    registration_vocabulary::{registration_vocabulary, vehicle_and_parking_registration},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::UpdateRegistration},
    model::{Context, NgsiLdMediaType},
    registration::RegistrationFragment,
};

/// Pins: Update Context Source Registration with a fragment setting `expiresAt` to the NGSI-LD Null
/// urn:ngsi-ld:null answers 204, and the registration then reads back without an `expiresAt`
/// Clause: 5.9.3, 5.10.1, 5.2.9, 5.5.8, 4.5.0
#[gtest]
#[tokio::test]
async fn n_csr_upd_0003_an_ngsi_ld_null_expires_at_removes_the_expiration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(&broker, &registration_vocabulary(&namespace)?).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let registration = vehicle_and_parking_registration(&namespace, &registration_id, Some("2035-11-29T14:53:15Z".parse()?))?;
    body_context.registrations().register(&registration).send().await?;
    let fragment = RegistrationFragment::builder().build()?;
    // The NGSI-LD Null is no value of a typed `expiresAt`, so the member is written as text.
    let document = UncheckedDocument::try_from(fragment.render())?.remove_member("type").null_member("expiresAt");
    let request = broker
        .unchecked(&UpdateRegistration {
            id: &registration_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.expires_at(), none());
    Ok(())
}
