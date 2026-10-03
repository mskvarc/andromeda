//! What a response names does not depend on which cache tier holds a
//! document (study A18).
//!
//! Whether a request context ends with the core `@context` is read by loading
//! the documents it references through the one pipeline processing uses
//! (in-process cache, registry, durable cache, network). A broker process that
//! has never held a document in memory, reading it back from the durable cache
//! only, names exactly the same value as the process that downloaded it.

use crate::{
    broker_endpoints::broker_url,
    context_api::cached_urls,
    context_links::{context_link, context_link_targets},
    ngsi_requests::create_device,
    polling::eventually,
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::UserContexts,
};
use andromeda_test_harness::broker_process::BrokerProcess;
use reqwest::{Client, header::HeaderMap};
use serde_json::Value;
use std::path::Path;

/// The `@context` an `application/ld+json` retrieval of `device` at
/// `entities_url` names, and the context link targets an `application/json`
/// one carries, with the scheme and authority removed from every target the
/// answering broker minted (it mints them from the origin it was addressed on).
async fn named_by(client: &Client, entities_url: &str, device: &str, link: &str) -> (Value, Vec<String>) {
    let url = format!("{entities_url}/{device}");
    let body: Value = client
        .get(&url)
        .header("Accept", "application/ld+json")
        .header("Link", context_link(link))
        .send()
        .await
        .expect("retrieval failed")
        .json()
        .await
        .expect("a JSON body");
    let headers: HeaderMap = client
        .get(&url)
        .header("Accept", "application/json")
        .header("Link", context_link(link))
        .send()
        .await
        .expect("retrieval failed")
        .headers()
        .clone(); // clone: the headers outlive the consumed response
    let targets = context_link_targets(&headers)
        .into_iter()
        .map(|target| match target.find("/ngsi-ld/v1/jsonldContexts/") {
            Some(start) => target[start..].to_owned(),
            None => target,
        })
        .collect();
    (body["@context"].clone(), targets) // clone: the body is dropped after this
}

/// The broker under test, and a broker process started afterwards on the same
/// database with the context origin already gone, name byte-identical values
/// for every shape of request context.
#[tokio::test]
async fn a_fresh_broker_process_reading_the_durable_cache_names_the_same_values() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:determinism");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let broker_entities = broker_url("entities").trim_end_matches('/').to_owned();
    // clone: the URLs outlive the origin, which is shut down before the fresh broker starts
    let shapes: Vec<String> = [&contexts.terms, &contexts.wrapper, &contexts.outer, &contexts.relative, &contexts.v18_tail]
        .into_iter()
        .cloned()
        .collect();

    let mut first = Vec::new();
    for shape in &shapes {
        first.push(named_by(tenant.client(), &broker_entities, &device, shape).await);
    }
    let client = tenant.client();
    let mut downloads = shapes.clone(); // clone: the polled list gains the nested documents
    downloads.push(contexts.origin().url("/user/nested/wrapped.jsonld"));
    let expected = &downloads;
    eventually("every download is recorded durably", move || async move {
        let cached = cached_urls(client).await;
        expected.iter().all(|url| cached.contains(url)).then_some(())
    })
    .await;
    contexts.shutdown().await;
    let fresh = BrokerProcess::start(Path::new(env!("CARGO_BIN_EXE_andromeda")), &[])
        .await
        .expect("the broker starts");

    for (shape, before) in shapes.iter().zip(first) {
        let after = named_by(tenant.client(), &fresh.url("entities"), &device, shape).await;
        assert_eq!(after, before, "{shape}");
    }
    drop(fresh);
    tenant.finish().await;
}
