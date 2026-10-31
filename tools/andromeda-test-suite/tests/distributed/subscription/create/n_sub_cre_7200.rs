//! A subscription created under a via header naming a registration alias is not forwarded to that source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Via, ViaEntry},
        operation::RegisterContextSource,
    },
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with a Via header listing the `contextSourceAlias` of a matching registration does not
/// forward the subscription to that source
/// Clause: 6.3.18, 5.12, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_7200_a_subscription_created_under_a_via_header_naming_a_registration_alias_is_not_forwarded_to_that_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([OperationGroup::FederationOps])
        .build()?;
    // The model refuses it (a registration contextSourceAlias), so `contextSourceAlias` is written as text.
    let context_source_alias: RawJson = format!(r#""cs-{namespace}""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceAlias", &context_source_alias);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker
        .with_header(Via::try_from_entries([format!("1.1 cs-{namespace}").parse::<ViaEntry>()?])?)?
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/subscriptions").await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
