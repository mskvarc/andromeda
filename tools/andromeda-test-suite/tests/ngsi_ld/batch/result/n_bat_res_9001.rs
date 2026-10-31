//! An unretrievable context is one error per entity in a 207.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    partial_outcome::PartialSuccess,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Creation of two application/ld+json entities each carrying an unretrievable @context of its
/// own answers 207 with no success and one `LdContextNotAvailable` error with a title against each entity, and
/// creates neither
/// Clause: 5.6.7.4, 5.6.7.5, 5.5.6, 5.2.17, 6.3.5
#[gtest]
#[tokio::test]
async fn n_bat_res_9001_an_unretrievable_context_is_one_error_per_entity_in_a_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let unreachable_body = broker
        .with_context(Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::new(format!(
            "{unreachable}/ctx/{namespace}/unreachable.jsonld"
        ))?)]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building_1_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Reichstag")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Reichstag")
            .build()?,
    ];
    let report = unreachable_body.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_1_id).problem(ErrorType::LdContextNotAvailable))
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_2_id).problem(ErrorType::LdContextNotAvailable))
    );
    for error in &report.errors {
        expect_that!(&error.error.title, some(anything()));
    }
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_1_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
