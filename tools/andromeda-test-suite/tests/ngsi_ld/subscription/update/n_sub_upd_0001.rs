//! Entities written without a context read back as short names.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: a fragment replacing entities, sent with no Link header, is expanded under the core @context, and a
/// retrieve with no Link header compacts the stored type back to the same short name
/// Clause: 5.8.2.4, 5.5.7, 5.5.8, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_upd_0001_entities_written_without_a_context_read_back_as_short_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().entities([vehicle_type.as_str()]).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.entities(), len(eq(1)));
    expect_that!(
        subscription_2.entities().iter().map(EntitySelector::selector_type).collect::<Vec<_>>(),
        nth(0, eq(&&SelectorTypeBuilder::from(&vehicle_type).build()?))
    );
    Ok(())
}
