//! ETSI `051_03`: a cached `@context` is deleted with 204 and an empty body when no `reload` is given (clause 5.13.5).

use crate::jsonld_context::{
    building_use::{serve_building_terms, use_hosted_context},
    stored_contexts::{add_context, delete_own_contexts, eventual_details},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry, ContextId};
use serde_json::json;

/// Pins: Check that one can delete a cached @context
/// Clause: 5.13.5
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by their host and path, as application/ld+json documents mapping the
/// terms the case uses to per-file IRIs
/// Changed: the request that reads a Cached @context right after its download is retried for up to 3 s (12 times
/// 250 ms), the wait the suite itself gives a download to be recorded in `051_05` and `053_05`: Andromeda records a
/// downloaded @context after the request that downloaded it
/// Changed: Delete Initial @context lists every @context of the tenant and deletes all but the core one, which in the
/// shared tenant would delete every other file's @contexts; the teardown still lists them and deletes only this file's
/// own (those it added and those its requests had downloaded), all the keyword deletes in a broker that holds nothing
/// else
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
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
    let cached = json!("Cached");
    let downloaded = eventual_details(&broker, &cached_id, |details| details.get("kind") == Some(&cached)).await?;
    expect_that!(downloaded.get("kind"), some(eq(&cached)));
    let deleted = broker.with_responses().contexts().delete(&cached_id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    expect_that!(deleted?.body, is_empty());
    delete_own_contexts(&broker, &[added.id, cached_id]).await?;
    remote.stop();
    Ok(())
}
