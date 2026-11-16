//! A ranked lang is updated as written and deleted by the ngsi ld null.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::language_filter::LanguageFilter,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember},
};

/// Pins: Update Subscription of a Subscription whose `lang` is en with a fragment whose `lang` is the ranked filter
/// `ja,en;q=0.8,*;q=0.1` answers 204, and Retrieve Subscription answers `lang` as `ja,en;q=0.8,*;q=0.1`; a second
/// update writing `lang` as the NGSI-LD Null answers 204, and the Subscription reads back without `lang`
/// Clause: 4.15, 5.5.8, 5.8.2.4, Table 5.2.12-1
/// Recorded: Table 5.2.12-1 types the Subscription `lang` as the language filter of clause 4.15, whose example 4 is
/// a ranked list, so an update takes what a creation takes, and clause 5.5.8 deletes a member the fragment writes as
/// the NGSI-LD Null
#[gtest]
#[tokio::test]
async fn n_sub_upd_9015_a_ranked_lang_is_updated_as_written_and_deleted_by_the_ngsi_ld_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("ranked-lang-update")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .lang("en".parse::<LanguageFilter>()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let ranked = SubscriptionFragment::builder().lang("ja,en;q=0.8,*;q=0.1".parse::<LanguageFilter>()?).build()?;
    let updated = broker.with_responses().subscriptions().update(&subscription_id, &ranked).send().await;
    expect_that!(&updated, answered(StatusCode::NO_CONTENT));
    let stored = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored.lang().map(ToString::to_string), some(eq("ja,en;q=0.8,*;q=0.1")));
    let removal = SubscriptionFragment::builder().delete(SubscriptionMember::Lang).build()?;
    let deleted = broker.with_responses().subscriptions().update(&subscription_id, &removal).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    let stored_2 = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored_2.lang(), none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
