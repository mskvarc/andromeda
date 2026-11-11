//! The created of a proof reads back with the digits it was written with.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Create Entity with three attributes whose ngsildproof `created` is written with three and with two fractional
/// digits ending in a zero, and with none, answers 201, and each `created` reads back with exactly the digits it was
/// written with: the RDF Dataset Canonicalization a proof is verified by keeps the lexical form of a literal, so
/// dropping a trailing zero would change the signed data
/// Clause: 4.5.2.2, 4.6.3, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_cmn_val_9039_the_created_of_a_proof_reads_back_with_the_digits_it_was_written_with() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "spelled")?;
    let building_type = namespace.entity_type("Building")?;
    let written = [
        ("millis", "2025-01-27T21:02:24.500Z", "2025-01-27T21:02:24.500Z"),
        ("centis", "2025-01-27T21:02:24.50Z", "2025-01-27T21:02:24.50Z"),
        ("whole", "2025-01-27T21:02:24Z", "2025-01-27T21:02:24Z"),
    ];
    let attributes = written
        .iter()
        .map(|(name, created, _)| {
            format!(
                r#""{name}": {{"type": "Property", "value": 1, "ngsildproof": {{"type": "Property", "entityIdSealed": "{building_id}", "entityTypeSealed": "{building_type}", "value": {{"type": "DataIntegrityProof", "created": "{created}", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx"}}}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    // The model writes no seal as a member of an attribute, so the body is sent as written.
    let request = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(r#"{{"id": "{building_id}", "type": "{building_type}", {attributes}}}"#),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker.entities().retrieve(&building_id).send().await?;
    let entity = retrieved.body();
    for (name, _, served) in written {
        expect_that!(entity, has_attribute(name, any_attribute().proof_member(["created"], served)), "{name}");
    }
    Ok(())
}
