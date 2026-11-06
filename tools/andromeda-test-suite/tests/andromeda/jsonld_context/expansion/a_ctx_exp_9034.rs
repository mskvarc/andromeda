//! A broker that fetches nothing resolves a user @context naming the Data Integrity context.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    data_integrity_proof::{proof_json, proof_value},
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntity},
    model::{Context, Entity, Lossless, NgsiLdMediaType, Proof, Property},
};
use serde_json::Value as Json;

/// Pins: On a broker with remote @context fetching disabled, Create Entity with a Link header naming
/// `https://w3id.org/security/data-integrity/v2`, of an Entity whose Property carries an ngsildproof written with the
/// terms of that @context, answers 201, and Retrieve Entity under the same Link header answers the proof value exactly
/// as written
/// Clause: 4.5.2.2, 5.5.7, 5.6.1, 5.7.1, 6.3.5
/// Recorded: Andromeda configuration, not v1.9.1: `jsonld.fetch.mode = "disabled"` fetches no @context; the W3C
/// Data Integrity v2 context is compiled into the broker, so a user @context naming it resolves without a network,
/// and it is part of the Core @context, whose scoped contexts clause 5.5.7 does not refuse
#[gtest]
#[tokio::test]
async fn a_ctx_exp_9034_a_broker_that_fetches_nothing_resolves_a_user_context_naming_the_data_integrity_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let linked =
        broker::client(BrokerRole::SingleTenant)?.with_context(Context::from(IriBuf::new("https://w3id.org/security/data-integrity/v2".to_owned())?))?;
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
    linked.entities().create(&store).send().await?.done()?;
    let retrieve = RetrieveEntity::builder().id(&store_id).format(Lossless::Normalized).build();
    let answer = send_versioned(&linked.with_accept(Accept::new(NgsiLdMediaType::Json)), &retrieve, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    let body: Json = serde_json::from_slice(&answer.body)?;
    expect_that!(body["address"]["ngsildproof"]["value"], eq(&proof_json()));
    Ok(())
}
