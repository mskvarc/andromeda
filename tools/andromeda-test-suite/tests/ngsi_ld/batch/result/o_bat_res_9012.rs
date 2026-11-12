//! Contexts written differently that name one unretrievable document download it once per batch.

use andromeda_test_suite::{
    broker,
    core_context::{core_context, core_entry},
    namespace::Namespace,
    partial_outcome::reported,
    problem::answered,
    remote_context::RemoteContexts,
    report_matcher::batch_error,
    unchecked_document::from_rendered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Context, Entity, ErrorType, Input, NgsiLdMediaType, Representation},
};
use std::result::Result as StdResult;

/// The path the context host serves nothing at, so a download of it answers 404.
const MISSING: &str = "jc/missing.jsonld";

/// Pins: Batch Entity Creation of five application/ld+json entities, each carrying its own @context naming one
/// document the context host answers 404 for, two of them written alike and three written each another way, answers
/// 207 with no success and a `LdContextNotAvailable` error against each entity after one download of the document;
/// the same batch sent again answers alike after one more download, and no entity is created
/// Clause: 5.6.7.4, 5.6.7.5, 5.5.6, 6.3.5
/// Recorded: clause 5.5.6 raises `LdContextNotAvailable` for a remote @context that is not available and leaves how
/// often it is retrieved to the implementation; Andromeda tries a document once per batch, however many contexts of
/// the batch name it, and tries it again in the next request
#[gtest]
#[tokio::test]
async fn o_bat_res_9012_contexts_written_differently_naming_one_unretrievable_document_download_it_once_per_batch() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let missing = contexts.reference(MISSING)?;
    let core = core_entry()?;
    let written = [
        Context::Single(missing.clone()),
        Context::Single(missing.clone()),
        Context::Multiple(vec![missing.clone()]),
        Context::Multiple(vec![missing.clone(), core.clone()]),
        Context::Multiple(vec![core, missing]),
    ];
    let building_type = namespace.entity_type("Building")?;
    let entities = written
        .into_iter()
        .enumerate()
        .map(|(index, context)| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", index)?)
                .entity_type(building_type.clone())
                .property("height", 1)
                .context(context)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let documents = entities
        .iter()
        .map(|entity| from_rendered(&entity.render(Representation::Normalized)))
        .collect::<StdResult<Vec<UncheckedDocument>, _>>()?;
    let ld_json_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let create = BatchCreate::builder().entities(&entities).build();
    for (attempt, downloads) in [(1, 1), (2, 2)] {
        let request = ld_json_body.unchecked(&create)?.documents(&documents);
        let report: BatchOperationResult = reported(ld_json_body.send_unchecked(request).await)?;
        expect_that!(&report.success, is_empty(), "batch {attempt}");
        expect_that!(&report.errors, len(eq(entities.len())), "batch {attempt}");
        for entity in &entities {
            expect_that!(
                &report.errors,
                contains(batch_error().entity(&entity.id).problem(ErrorType::LdContextNotAvailable)),
                "batch {attempt}"
            );
        }
        expect_that!(contexts.downloads(MISSING).await?, eq(downloads), "after batch {attempt}");
    }
    for entity in &entities {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&entity.id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
