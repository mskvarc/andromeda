//! A proof written with W3C terms reads back as written whatever the user context defines.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    data_integrity_proof::{proof_json, proof_value, shadowing_terms},
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::RetrieveEntity},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType, Proof, Property},
};
use serde_json::Value as Json;

/// Pins: An Entity created under a user @context defining every term of its proof under another vocabulary, whose
/// Property carries an ngsildproof written with the terms of the W3C Data Integrity v2 context, is retrieved without
/// that @context with the proof value exactly as written: inside ngsildproof the W3C terms win, so its members expand
/// to their W3C IRIs and not to the ones the writer's @context gives them
/// Clause: 4.5.2.2, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9041_a_proof_written_with_w3c_terms_reads_back_as_written_whatever_the_user_context_defines() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = format!("https://example.org/{namespace}/vocab/");
    let writer = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(shadowing_terms(&vocabulary).parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let store_id = namespace.urn("Store", 1)?;
    let proof = Proof::builder(proof_value())
        .entity_id_sealed(store_id.as_str())
        .entity_type_sealed(namespace.suffixed("Store").as_str())
        .build()?;
    let store = Entity::builder()
        .id(store_id.clone())
        .entity_type(namespace.entity_type("Store")?)
        .property("address", Property::builder("Tiger Street 4").proof(proof))
        .build()?;
    writer.entities().create(&store).send().await?.done()?;
    let retrieve = RetrieveEntity::builder().id(&store_id).format(Lossless::Normalized).build();
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &retrieve, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    let body: Json = serde_json::from_slice(&answer.body)?;
    expect_that!(body["address"]["ngsildproof"]["value"], eq(&proof_json()));
    Ok(())
}
