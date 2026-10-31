//! A matching subscription places a reduced copy on the supporting source pointing at this broker.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    config::PeerRole,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySelector, SelectorType, type_selection},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription over an inclusive registration holding `federationOps` places one Subscription on the
/// source with the entity selector of the registration, a notification endpoint on this broker and not the
/// subscriber sink
/// Clause: 5.8.1.4, 4.20
#[gtest]
#[tokio::test]
async fn n_sub_cre_6805_a_matching_subscription_places_a_reduced_copy_on_the_supporting_source_pointing_at_this_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    source
        .answer(Method::POST, "/s1/ngsi-ld/v1/subscriptions", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer_when(
            SourceRequest::any_matching("/s1/ngsi-ld/v1/subscriptions/.*"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let forwarded_request = SourceRequest::new(Method::POST, "/s1/ngsi-ld/v1/subscriptions").body(BodyPattern::SelectsBelow(
        "$.entities".parse()?,
        format!("$[?(@.type == 'https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}')]").parse()?,
    ));
    let forwarded = source.wait_for_matching(&forwarded_request, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_subscription = forwarded_body::subscription(forwarded_one)?;
    expect_that!(forwarded_subscription.id().map(ToString::to_string), some(starts_with("urn:")));
    expect_that!(
        forwarded_subscription.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(
            0,
            eq(&&SelectorType::Selection(
                type_selection::entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}").as_str()).build()?
            ))
        )
    );
    expect_that!(
        forwarded_subscription.notification().endpoint().uri().as_str(),
        starts_with(broker::peer(PeerRole::A)?.as_str())
    );
    expect_that!(forwarded_subscription.notification().endpoint().uri(), not(eq(&sink.endpoint(1)?)));
    Ok(())
}
