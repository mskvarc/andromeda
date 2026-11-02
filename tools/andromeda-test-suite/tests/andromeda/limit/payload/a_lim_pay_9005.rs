//! A batch refuses only its entity over the budget.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::ring_polygon,
    identity_matcher::has_id,
    namespace::Namespace,
    partial_outcome::reported,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Batch upsert of an Entity whose geometry is one position over the budget beside a small Entity answers
/// 207, writing the small one and reporting the other with a 413 `RequestEntityTooLarge`
/// Clause: 6.3.2, 5.6.8, 5.2.17
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9005_a_batch_refuses_only_its_entity_over_the_budget() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let big_id = namespace.urn("Probe", "batch-big")?;
    let small_id = namespace.urn("Probe", "batch-small")?;
    let entities = [
        Entity::builder()
            .id(big_id.clone())
            .entity_type(probe_type.clone())
            .geo_property("location", ring_polygon(budget + 1)?)
            .build()?,
        Entity::builder()
            .id(small_id.clone())
            .entity_type(probe_type.clone())
            .property("n", Property::builder(1))
            .build()?,
    ];
    let answer = broker
        .send_unchecked(broker.unchecked(&BatchUpsert::builder().entities(&entities).build())?)
        .await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(report.success, elements_are![eq(&small_id)]);
    expect_that!(report.errors, len(eq(1)));
    let failure = report.errors.first().or_fail()?;
    expect_that!(failure.entity_id.to_string(), eq(&big_id.to_string()));
    expect_that!(failure.error.status, some(eq(413)));
    expect_that!(
        failure.error.error_type.as_ref().map(ToString::to_string),
        some(eq("https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge"))
    );
    let small = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&small_id)
        .send()
        .await?;
    expect_that!(small.body(), has_id(&small_id));
    let big = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&big_id)
        .send()
        .await;
    expect_that!(&big, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
