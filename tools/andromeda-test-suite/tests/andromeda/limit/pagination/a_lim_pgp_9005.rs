//! A page that renders past the page byte ceiling is 403 `TooManyResults`, and one under it is served.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Entities the collection holds; their rendered size is what the ceiling is held against.
const ENTITIES: u32 = 9;

/// Bytes of text in the one Property of each Entity, under the 1,000,000 an entity may hold.
const BLOB_BYTES: usize = 935_000;

/// Pins: Query Entities over nine Entities of 935,000 bytes each answers 200 with limit 8, a page of about 7.5 MB
/// under the ceiling of 8 MiB, and answers 403 `TooManyResults` naming the ceiling with limit 9, a page of about
/// 8.4 MB over it
/// Clause: 5.5.6, 5.5.9.1
/// Recorded: clause 5.5.6 leaves the `TooManyResults` threshold to the broker; Andromeda refuses a page whose
/// rendered body exceeds `request_limits.max_page_body` (8 MiB on this stack), and clause 5.5.9.1 makes the page size
/// the requested limit, so a page over the ceiling is refused rather than served smaller
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9005_a_page_over_the_byte_ceiling_is_too_many_results_and_one_under_it_is_served() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ceiling = SuiteConfig::get()?.budget(Budget::PageBytes)?;
    let blob_type = namespace.entity_type("Blob")?;
    for index in 0..ENTITIES {
        let entity = Entity::builder()
            .id(namespace.urn("Blob", format!("{index:04}"))?)
            .entity_type(blob_type.clone())
            .property("blob", "x".repeat(BLOB_BYTES))
            .build()?;
        broker.entities().create(&entity).send().await?;
    }
    let under = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&blob_type)
        .limit(ENTITIES - 1)
        .send()
        .await;
    expect_that!(&under, answered(StatusCode::OK));
    let over = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&blob_type)
        .limit(ENTITIES)
        .send()
        .await;
    expect_that!(
        &over,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing(&format!("more than the {ceiling} bytes one page may hold"))
    );
    Ok(())
}
