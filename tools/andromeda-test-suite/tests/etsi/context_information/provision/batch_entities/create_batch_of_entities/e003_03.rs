//! ETSI `003_03`: a batch whose body is not JSON, or is an empty array, is refused as a whole (clause 5.6.7).

use crate::{
    suite_keywords::{post_text, refused, with_doubled_separator},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{
    http::{ContextDelivery, operation::BatchCreate},
    model::{APPLICATION_LD_JSON, Context, ContextEntry, ErrorType},
};

/// The `@context` of `vocabulary` followed by the ETSI core context v1.8, the two references the invalid body names.
fn vocabulary_then_core(vocabulary: &Vocabulary) -> Result<Context> {
    let core: IriBuf = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".parse()?;
    // The context owns its references, and the vocabulary keeps its URL for later requests.
    Ok(Context::Multiple(vec![
        ContextEntry::Reference(IriRefBuf::from(vocabulary.url().clone())),
        ContextEntry::Reference(IriRefBuf::from(core)),
    ]))
}

/// Pins: Check that one cannot create a batch of entities with an invalid request
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_03_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = broker
        .with_context(vocabulary_then_core(&vocabulary)?)?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let entities = [bare_building(&namespace, "randomUUID")?];
    let operation = BatchCreate::builder().entities(&entities).build();
    let request = with_doubled_separator(client.unchecked(&operation)?);
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}

/// Pins: Check that one cannot create a batch of entities with an invalid request
/// Clause: 5.6.7
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e003_03_02_empty_json() -> Result<()> {
    let broker = broker::front()?;
    refused(
        &broker,
        post_text(&broker, "entityOperations/create", &APPLICATION_LD_JSON, "[]")?,
        ErrorType::BadRequestData,
    )
    .await
}
