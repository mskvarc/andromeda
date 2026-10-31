//! As json ld every broker embeds the context.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentType},
    model::NgsiLdMediaType,
};

/// Pins: Retrieve Context Source Identity Information with Accept application/ld+json on broker A, broker B and
/// broker C answers 200 as application/ld+json with an @context member
/// Clause: 5.15.1.4, 6.3.6, 6.3.5
#[gtest]
#[tokio::test]
async fn n_inf_ret_7601_as_json_ld_every_broker_embeds_the_context() -> Result<()> {
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    let identity = &headed.outcome;
    expect_that!(identity.context.as_ref(), some(anything()));
    let headed_2 = broker_b
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed_2.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    let identity_2 = &headed_2.outcome;
    expect_that!(identity_2.context.as_ref(), some(anything()));
    let headed_3 = broker_c
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed_3.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    let identity_3 = &headed_3.outcome;
    expect_that!(identity_3.context.as_ref(), some(anything()));
    Ok(())
}
