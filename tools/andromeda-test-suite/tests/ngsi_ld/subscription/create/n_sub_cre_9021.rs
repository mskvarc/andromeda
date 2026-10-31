//! A subscription spelled with at id and at type is created.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose id, type and entity selector type are written as @id and @type answers 201, and
/// the Subscription reads back with id and type
/// Clause: 4.4, 5.8.1
#[gtest]
#[tokio::test]
async fn n_sub_cre_9021_a_subscription_spelled_with_at_id_and_at_type_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let keywords_id = namespace.subscription_id("keywords")?;
    let device_type = namespace.entity_type("Device")?;
    let subscription = Subscription::builder()
        .id(keywords_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model writes only `id` and `type`, so the keyword spellings are set on the rendered document.
    let selectors: RawJson = format!(r#"[{{"@type": "{device_type}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?
        .rename_member("id", "@id")
        .rename_member("type", "@type")
        .set_member("entities", &selectors);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&keywords_id)
        .send()
        .await?;
    expect_that!(stored.id(), eq(&keywords_id));
    expect_that!(
        stored.entities().first().map(EntitySelector::selector_type),
        some(eq(&SelectorTypeBuilder::from(&device_type).build()?))
    );
    broker.subscriptions().delete(&keywords_id).send().await?;
    Ok(())
}
