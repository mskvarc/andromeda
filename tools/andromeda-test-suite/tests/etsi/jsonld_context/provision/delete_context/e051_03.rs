//! A cached `@context` is deleted with 204 and an empty body when no `reload` is given (clause 5.13.5).

use crate::jsonld_context::{
    building_use::{serve_building_terms, use_hosted_context},
    stored_contexts::{add_context, delete_own_contexts, eventual_details},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry, ContextId, StoredContextKind};

/// Pins: Check that one can delete a cached @context
/// Clause: 5.13.5
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Timing: the request that reads a Cached @context right after its download is retried for up to 3 s (12 times 250
/// ms): Andromeda records a downloaded @context after the request that downloaded it
/// Isolation: the teardown lists the tenant's @contexts and deletes only this file's own (those it added and those its
/// requests downloaded); in the shared tenant the other @contexts belong to other files
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_03_01_delete_a_context_whose_kind_is_cached_without_reload_param() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let smart_data_models = serve_building_terms(&remote, &namespace, "smartdatamodels.org/context.jsonld").await?;
    let context = Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::new(smart_data_models.to_string())?)]);
    let added = add_context(&broker, &context).await?;
    use_hosted_context(&broker, &namespace, &added.location).await?;
    let cached_id: ContextId = smart_data_models.as_str().parse()?;
    let downloaded = eventual_details(&broker, &cached_id, |details| details.kind == StoredContextKind::Cached).await?;
    expect_that!(downloaded.kind, eq(&StoredContextKind::Cached));
    let deleted = broker.with_responses().contexts().delete(&cached_id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    expect_that!(deleted?.body, is_empty());
    delete_own_contexts(&broker, &[added.id, cached_id]).await?;
    remote.stop();
    Ok(())
}
