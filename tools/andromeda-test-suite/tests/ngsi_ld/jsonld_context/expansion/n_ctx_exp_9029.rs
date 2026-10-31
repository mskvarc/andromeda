//! A link parameter holding an escaped quote still names the context.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, remote_context::RemoteContexts, wire::request_to};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept},
    model::{Context, Entity, EntityTypeName, Lossless, NgsiLdMediaType, OneOrMany, Value},
};

/// Pins: Create Entity under a Link header whose title parameter holds an escaped quote and a semicolon before the
/// rel parameter expands the type by the linked context, so the type reads back as its full IRI without a context
/// and as the term under it
/// Clause: 6.3.5, 5.6.1; IETF RFC 8288 section 3, IETF RFC 7230 section 3.2.6
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9029_a_link_parameter_holding_an_escaped_quote_still_names_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "contexts/suite",
            &format!(r#"{{"@context": {{"Depot_{namespace}": "https://test.andromeda/{namespace}/Depot"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let served = contexts.url("contexts/suite")?;
    let building_id = namespace.urn("Building", "escaped-quote")?;
    let depot_type = namespace.entity_type("Depot")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(depot_type.clone())
        .property("temperature", Value::Number("19.0".parse()?))
        .build()?;
    // The title parameter holds an escaped quote ahead of the rel parameter, which no typed Link writes.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(
                r#"<{served}>; title="a\"b; rel=nonsense"; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
            ))?,
        )
        .document(&UncheckedDocument::from_entity(&building, Lossless::Normalized));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body().entity_type,
        eq(&OneOrMany::one(format!("https://test.andromeda/{namespace}/Depot").parse::<EntityTypeName>()?))
    );
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/suite")?))?;
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(retrieved_2.body().entity_type, eq(&OneOrMany::one(depot_type)));
    Ok(())
}
