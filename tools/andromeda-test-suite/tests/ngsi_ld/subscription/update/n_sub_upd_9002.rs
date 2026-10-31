//! A geo q reads back as written and the ngsi ld null removes it.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember},
};

/// Pins: A Subscription with `geoQ` near;maxDistance==2000 around a Point reads back as written, and Update
/// Subscription with `geoQ` written as the NGSI-LD Null removes `geoQ`
/// Clause: Table 5.2.12-1, Table 5.2.13-1, 5.5.8, 5.8.2
#[gtest]
#[tokio::test]
async fn n_sub_upd_9002_a_geo_q_reads_back_as_written_and_the_ngsi_ld_null_removes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("geo-q")?;
    let probe_type = namespace.entity_type("GeoProbe")?;
    let reference = Shape::Point(Position::try_new(14.5, 46.05)?);
    let geo_q = geoquery::near_max(2000, reference.clone()).build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .geo_q(geo_q.clone())
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let stored = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored.geo_q(), some(eq(&geo_q)));
    expect_that!(stored.geo_q().map(|query| query.relation().to_string()), some(eq("near;maxDistance==2000")));
    expect_that!(stored.geo_q().map(geoquery::GeoQuery::geometry), some(eq(&reference)));
    expect_that!(stored.geo_q().and_then(geoquery::GeoQuery::geoproperty), none());
    let removal = SubscriptionFragment::builder().delete(SubscriptionMember::GeoQ).build()?;
    broker.subscriptions().update(&subscription_id, &removal).send().await?;
    let stored_2 = json.subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(stored_2.geo_q(), none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
