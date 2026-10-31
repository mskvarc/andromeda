//! As json ld embeds the context.

use andromeda_test_suite::{broker, core_context::core_entry};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentType},
    model::{Context, NgsiLdMediaType},
};

/// Pins: Retrieve Context Source Identity Information as application/ld+json embeds the core @context in the body
/// Clause: 6.3.6, 5.15.1, 6.33.3.1
#[gtest]
#[tokio::test]
async fn n_inf_ret_0003_as_json_ld_embeds_the_context() -> Result<()> {
    let broker = broker::front()?;
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
    expect_that!(identity.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    Ok(())
}
