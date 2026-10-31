//! A hosted `@context` that lists remote URLs makes the broker cache each of them once a request uses it (clauses
//! 5.13.2 and 5.13.3).

use crate::jsonld_context::{
    building_use::{serve_building_terms, use_hosted_context},
    stored_contexts::{add_hosted_context, delete_own_contexts, eventual_details},
};
use andromeda_test_suite::{broker, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry, ContextId, StoredContextKind};

/// Pins: Check that one can add a @context
/// Clause: 5.13.2
/// Isolation: the remote @contexts are served per file under the mock origin, at `contexts/<ns>/` followed by their
/// host and path, as application/ld+json documents mapping the terms the test uses to per-file IRIs
/// Timing: the request that reads a Cached @context right after its download is retried for up to 3 s (12 times 250
/// ms): Andromeda records a downloaded @context after the request that downloaded it
/// Isolation: the teardown lists the tenant's @contexts and deletes only this file's own (those it added and those its
/// requests downloaded); in the shared tenant the other @contexts belong to other files
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e050_04_01_add_a_hosted_context_with_uris_and_check_they_are_cached() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let smart_data_models = serve_building_terms(&remote, &namespace, "smartdatamodels.org/context.jsonld").await?;
    let data_models = serve_building_terms(&remote, &namespace, "fiware.github.io/data-models/context.jsonld").await?;
    let context = Context::Multiple(vec![
        ContextEntry::Reference(IriRefBuf::new(smart_data_models.to_string())?),
        ContextEntry::Reference(IriRefBuf::new(data_models.to_string())?),
    ]);
    let added = add_hosted_context(&broker, &context).await?;
    use_hosted_context(&broker, &namespace, &added.location).await?;
    let smart_data_models_id: ContextId = smart_data_models.as_str().parse()?;
    let data_models_id: ContextId = data_models.as_str().parse()?;
    let first = eventual_details(&broker, &smart_data_models_id, |details| details.kind == StoredContextKind::Cached).await?;
    expect_that!(first.kind, eq(&StoredContextKind::Cached));
    let second = eventual_details(&broker, &data_models_id, |details| details.kind == StoredContextKind::Cached).await?;
    expect_that!(second.kind, eq(&StoredContextKind::Cached));
    delete_own_contexts(&broker, &[added.id, smart_data_models_id, data_models_id]).await?;
    remote.stop();
    Ok(())
}
