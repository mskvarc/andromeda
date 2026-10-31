//! A parenthesised scope query conjunction reads back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription created with the `scopeQ` (/Madrid;/Barcelona) is retrieved with `scopeQ`
/// (/Madrid;/Barcelona)
/// Clause: 4.19, 5.2.12, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9001_a_parenthesised_scope_query_conjunction_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_scope_conjunction_id = namespace.subscription_id("scope-conjunction")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_scope_conjunction = Subscription::builder()
        .id(subscription_scope_conjunction_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription scopeQ), so `scopeQ` is written as text.
    let scope_q: RawJson = r#""(/Madrid;/Barcelona)""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_scope_conjunction.render())?.set_member("scopeQ", &scope_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_scope_conjunction,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_scope_conjunction_id)
        .send()
        .await?;
    expect_that!(subscription.scope_q().map(ToString::to_string), some(eq("(/Madrid;/Barcelona)")));
    broker.subscriptions().delete(&subscription_scope_conjunction_id).send().await?;
    Ok(())
}
