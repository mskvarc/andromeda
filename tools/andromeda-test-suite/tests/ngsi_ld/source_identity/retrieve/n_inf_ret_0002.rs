//! As json links the context and embeds none.

use andromeda_test_suite::{broker, core_context::core_context_link, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, ContentType, Link},
    model::NgsiLdMediaType,
};

/// Pins: Retrieve Context Source Identity Information as application/json carries the core @context in a Link
/// header and no @context in the body
/// Clause: 6.3.6, 5.15.1, 6.33.3.1
#[gtest]
#[tokio::test]
async fn n_inf_ret_0002_as_json_links_the_context_and_embeds_none() -> Result<()> {
    let broker = broker::front()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
    );
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed.head.typed::<Link>().valid(), some(eq(&core_context_link()?)));
    let identity = &headed.outcome;
    expect_that!(identity.context.as_ref(), none());
    Ok(())
}
