//! ETSI `050_04`: a hosted `@context` that lists remote URLs makes the broker cache each of them once a request uses
//! it (clauses 5.13.2 and 5.13.3).

use crate::jsonld_context::{
    building_use::{serve_building_terms, use_hosted_context},
    stored_contexts::{add_hosted_context, delete_own_contexts, eventual_details},
};
use andromeda_test_suite::{broker, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry, ContextId};
use serde_json::json;

/// Pins: Check that one can add a @context
/// Clause: 5.13.2
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
    let cached = json!("Cached");
    let first = eventual_details(&broker, &smart_data_models_id, |details| details.get("kind") == Some(&cached)).await?;
    expect_that!(first.get("kind"), some(eq(&cached)));
    let second = eventual_details(&broker, &data_models_id, |details| details.get("kind") == Some(&cached)).await?;
    expect_that!(second.get("kind"), some(eq(&cached)));
    delete_own_contexts(&broker, &[added.id, smart_data_models_id, data_models_id]).await?;
    remote.stop();
    Ok(())
}
