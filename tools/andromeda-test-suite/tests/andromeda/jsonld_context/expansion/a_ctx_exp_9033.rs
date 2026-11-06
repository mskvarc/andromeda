//! A broker that fetches nothing keeps a W3C proof unchanged in the Temporal Evolution.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    core_context::core_entry,
    data_integrity_proof::{proof_json, proof_value, shadowing_terms},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, ContextEntry, Entity, InlineContext, Proof, Property},
};
use serde_json::Value as Json;

/// Pins: On a broker with remote @context fetching disabled, an Entity created under a user @context defining every
/// term of its proof under another vocabulary, whose Property observed at 2026-07-01 carries an ngsildproof written
/// with the terms of the W3C Data Integrity v2 context, has a Temporal Evolution retrieved without that @context whose
/// one instance carries the proof value exactly as written
/// Clause: 4.5.2.2, 5.5.7, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: `jsonld.fetch.mode = "disabled"` fetches no @context; the W3C
/// Data Integrity v2 context the Core @context names is compiled into the broker, so the proof members expand to
/// their W3C IRIs without a network
#[gtest]
#[tokio::test]
async fn a_ctx_exp_9033_a_broker_that_fetches_nothing_keeps_a_w3c_proof_unchanged_in_the_temporal_evolution() -> Result<()> {
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
        .property(
            "address",
            Property::builder("Tiger Street 4").observed_at("2026-07-01T00:00:00Z".parse()?).proof(proof),
        )
        .build()?;
    writer.entities().create(&store).send().await?.done()?;
    let request = wire::request_to(
        &offline,
        Method::GET,
        &format!("temporal/entities/{store_id}?timerel=after&timeAt=2026-01-01T00:00:00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = offline.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    let body: Json = serde_json::from_slice(answer?.body())?;
    let evolution = &body["address"];
    let instances = evolution.as_array().map_or_else(|| vec![evolution], |instances| instances.iter().collect());
    expect_that!(instances, len(eq(1)));
    expect_that!(instances.first().map(|instance| &instance["ngsildproof"]["value"]), some(eq(&proof_json())));
    Ok(())
}
