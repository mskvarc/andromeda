//! A broker that fetches nothing reads a W3C proof back as written in every representation.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    core_context::core_entry,
    data_integrity_proof::{proof_json, proof_value, shadowing_terms},
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{RetrieveEntity, Simplified},
    },
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType, Proof, Property},
};
use serde_json::Value as Json;

/// Pins: On a broker with remote @context fetching disabled, an Entity created under a user @context defining every
/// term of its proof under another vocabulary, whose Property carries an ngsildproof written with the terms of the W3C
/// Data Integrity v2 context, is retrieved without that @context with the proof value exactly as written in the
/// normalized and concise representations, and with the Property's value alone in the simplified one
/// Clause: 4.5.2.2, 4.5.4, 5.5.7, 5.7.1
/// Recorded: Andromeda configuration, not v1.9.1: `jsonld.fetch.mode = "disabled"` fetches no @context; the W3C
/// Data Integrity v2 context the Core @context names is compiled into the broker, so the proof members expand to
/// their W3C IRIs without a network
#[gtest]
#[tokio::test]
async fn a_ctx_exp_9032_a_broker_that_fetches_nothing_reads_a_w3c_proof_back_as_written_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let offline = broker::client(BrokerRole::SingleTenant)?;
    let vocabulary = format!("https://example.org/{namespace}/vocab/");
    let writer = offline
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
    let reader = offline.with_accept(Accept::new(NgsiLdMediaType::Json));
    for format in [Lossless::Normalized, Lossless::Concise] {
        let retrieve = RetrieveEntity::builder().id(&store_id).format(format).build();
        let answer = send_versioned(&reader, &retrieve, None).await?;
        expect_that!(answer.status, eq(StatusCode::OK));
        let body: Json = serde_json::from_slice(&answer.body)?;
        expect_that!(body["address"]["ngsildproof"]["value"], eq(&proof_json()), "{format:?}");
    }
    let simplified = RetrieveEntity::builder().id(&store_id).format(Simplified).build();
    let answer = send_versioned(&reader, &simplified, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    let body: Json = serde_json::from_slice(&answer.body)?;
    expect_that!(body["address"], eq(&Json::from("Tiger Street 4")));
    Ok(())
}
