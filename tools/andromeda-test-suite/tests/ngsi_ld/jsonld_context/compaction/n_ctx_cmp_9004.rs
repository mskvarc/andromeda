//! A context without the term leaves the name fully qualified when simplified.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
    simplified_matcher::{has_value, lacks_value},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with format=simplified under a @context declaring none of its terms answers `testedAt` and
/// `brandName` under their full URIs and not by a short name
/// Clause: C.7, 4.5.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_9004_a_context_without_the_term_leaves_the_name_fully_qualified_when_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/coercing.jsonld")?))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked_2 = broker.with_context(Context::Single(contexts.reference("ctx/silent.jsonld")?))?;
    contexts
        .serve(
            "ctx/coercing.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Vehicle_{namespace}": "http://example.org/{namespace}/Vehicle",
        "brandName": "http://example.org/{namespace}/brandName",
        "testedAt": {{
          "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
          "@id": "http://example.org/{namespace}/testedAt"
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("testedAt", "2018-12-04T12:00:00Z")
        .property("brandName", "Mercedes")
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    contexts
        .serve(
            "ctx/silent.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "unrelated": "http://example.org/{namespace}/unrelated"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let retrieved = linked_2
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_value(&format!("http://example.org/{namespace}/testedAt"), "2018-12-04T12:00:00Z"));
    expect_that!(entity, lacks_value("testedAt"));
    expect_that!(entity, has_value(&format!("http://example.org/{namespace}/brandName"), "Mercedes"));
    Ok(())
}
