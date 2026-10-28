//! ETSI `050_01`: a `@context` added as a term object or as a list of remote URLs is stored as `Hosted`, answered with
//! 201, an empty body and a `Location` (clause 5.13.2).

use crate::jsonld_context::stored_contexts::{add_hosted_context, attribute_terms, delete_own_contexts};
use andromeda_test_suite::{broker, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry};

/// Pins: Check that one can add a @context
/// Clause: 5.13.2
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
async fn e050_01_01_add_a_valid_hosted_context_from_key_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_hosted_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    delete_own_contexts(&broker, &[added.id]).await
}

/// Pins: Check that one can add a @context
/// Clause: 5.13.2
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by their host and path, as application/ld+json documents mapping the
/// terms the case uses to per-file IRIs
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
async fn e050_01_02_add_a_valid_cached_context_from_uri() -> Result<()> {
    let broker = broker::front()?;
    let remote = RemoteContexts::start().await?;
    let smart_data_models = remote
        .serve("smartdatamodels.org/context.jsonld", r#"{"@context": {}}"#, Some("application/ld+json"))
        .await?;
    let data_models = remote
        .serve(
            "fiware.github.io/data-models/context.jsonld",
            r#"{"@context": {}}"#,
            Some("application/ld+json"),
        )
        .await?;
    let context = Context::Multiple(vec![
        ContextEntry::Reference(IriRefBuf::new(smart_data_models.to_string())?),
        ContextEntry::Reference(IriRefBuf::new(data_models.to_string())?),
    ]);
    let added = add_hosted_context(&broker, &context).await?;
    delete_own_contexts(&broker, &[added.id, smart_data_models.as_str().parse()?, data_models.as_str().parse()?]).await?;
    remote.stop();
    Ok(())
}
