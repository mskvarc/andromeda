//! A subscription is retrieved from the broker alone beside a registration advertising the retrieve operation through its group.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    federation_ops_source::{expect_unreached, register_federation_ops_source},
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::accept_copies,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Subscription beside a registration advertising `federationOps`, which holds
/// `retrieveSubscription`, answers the subscription the broker holds and sends no read to the source, whose copy is
/// placed by a create only
/// Clause: 5.8.3.4, 4.20
#[gtest]
#[tokio::test]
async fn n_sub_ret_9119_a_subscription_is_retrieved_from_the_broker_alone_beside_a_registration_advertising_retrieve_subscription_through_its_group()
-> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    accept_copies(&source, "a").await?;
    register_federation_ops_source(&broker, &source, &namespace).await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(retrieved.id(), eq(&subscription_id));
    expect_unreached(&source, &SourceRequest::matching(Method::GET, "/a/.*")).await?;
    Ok(())
}
