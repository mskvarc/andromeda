//! Batch items naming different served contexts are each expanded through their own.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Creation as application/ld+json whose two items name two different remote @contexts stores
/// each item's speed under the IRI its own @context gives
/// Clause: 6.3.5, 5.6.7, JSON-LD 1.1 s3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0029_batch_items_naming_different_served_contexts_are_each_expanded_through_their_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            &format!("jc/{namespace}/batch/a.jsonld"),
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/a/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            &format!("jc/{namespace}/batch/b.jsonld"),
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/b/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicles = [
        Entity::builder()
            .id(namespace.urn("Vehicle", 1)?)
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Vehicle", 2)?)
            .entity_type(vehicle_type)
            .property("speed", 2)
            .build()?,
    ];
    let documents: Vec<UncheckedDocument> = vehicles
        .iter()
        .zip(["a", "b"])
        .map(|(vehicle, served)| {
            let reference = contexts.url(&format!("jc/{namespace}/batch/{served}.jsonld"))?;
            Ok(UncheckedDocument::from_entity(vehicle, Lossless::Normalized).set_text("@context", reference.as_str()))
        })
        .collect::<Result<_>>()?;
    // The model writes one @context into the whole batch body; each item names its own, so the documents set it.
    let body_context = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let request = body_context
        .unchecked(&BatchCreate::builder().entities(&vehicles).build())?
        .documents(&documents);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&namespace.urn("Vehicle", "1")?)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/a/speed"), any_attribute().value(1))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&namespace.urn("Vehicle", "2")?)
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/b/speed"), any_attribute().value(2))
    );
    Ok(())
}
