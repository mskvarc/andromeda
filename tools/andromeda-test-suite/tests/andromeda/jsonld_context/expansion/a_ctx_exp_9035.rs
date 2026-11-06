//! The Data Integrity context is neither listed nor served after use.

use andromeda_test_suite::{broker, config::BrokerRole, data_integrity_proof::proof_value, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextId, Entity, ErrorType, Proof, Property},
};
use url::Url;

/// The URL the W3C publishes the Data Integrity v2 @context under.
const DATA_INTEGRITY_CONTEXT: &str = "https://w3id.org/security/data-integrity/v2";

/// Pins: On a broker with remote @context fetching disabled, after Create Entity under a Link header naming the W3C
/// Data Integrity v2 @context, List @contexts with kind=Cached does not list its URL, and Serve @context of it
/// answers 404 `ResourceNotFound`, content and details alike
/// Clause: 5.13.3.4, 5.13.4.4, 4.4
/// Recorded: Andromeda configuration, not v1.9.1: `jsonld.fetch.mode = "disabled"` fetches no @context; the W3C
/// Data Integrity v2 context is compiled into the broker like the Core @context, so it is no stored entry and its
/// resolution records no Cached one
#[gtest]
#[tokio::test]
async fn a_ctx_exp_9035_the_data_integrity_context_is_neither_listed_nor_served_after_use() -> Result<()> {
    let namespace = Namespace::fresh();
    let offline = broker::client(BrokerRole::SingleTenant)?;
    let linked = offline.with_context(Context::from(IriBuf::new(DATA_INTEGRITY_CONTEXT.to_owned())?))?;
    let store_id = namespace.urn("Store", 1)?;
    let proof = Proof::builder(proof_value())
        .entity_id_sealed(store_id.as_str())
        .entity_type_sealed(namespace.suffixed("Store").as_str())
        .build()?;
    let store = Entity::builder()
        .id(store_id)
        .entity_type(namespace.entity_type("Store")?)
        .property("address", Property::builder("Tiger Street 4").proof(proof))
        .build()?;
    linked.entities().create(&store).send().await?.done()?;
    let cached = offline
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .send()
        .await?;
    expect_that!(cached, not(contains(eq(&Url::parse(DATA_INTEGRITY_CONTEXT)?))));
    let context_id = DATA_INTEGRITY_CONTEXT.parse::<ContextId>()?;
    let served = offline.with_responses().with_accept(Accept::any()).contexts().serve(&context_id).send().await;
    expect_that!(&served, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let details = offline
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve_details(&context_id)
        .send()
        .await;
    expect_that!(&details, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
