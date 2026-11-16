//! A subscription created with a ranked lang reads the ranked filter back.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::language_filter::LanguageFilter,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `lang` is the ranked filter `fr-CH,fr;q=0.9,*;q=0.5` is created, and Retrieve
/// Subscription answers `lang` as `fr-CH,fr;q=0.9,*;q=0.5`, the ranges in the order the client wrote them
/// Clause: 4.15, Table 5.2.12-1, 5.8.1
/// Recorded: Table 5.2.12-1 types the Subscription `lang` as the language filter of clause 4.15, whose example 4 is
/// a ranked list, so the member takes what the `lang` query parameter takes
#[gtest]
#[tokio::test]
async fn n_sub_cre_9035_a_subscription_created_with_a_ranked_lang_reads_the_ranked_filter_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id("ranked-lang")?;
    let lang: LanguageFilter = "fr-CH,fr;q=0.9,*;q=0.5".parse()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .lang(lang)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.lang().map(ToString::to_string), some(eq("fr-CH,fr;q=0.9,*;q=0.5")));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
