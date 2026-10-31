//! A query answers the same names as a retrieval.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities under a @context coercing `testedAt` answers the one Entity with `testedAt` by its short
/// name with format=simplified and under its full URI in the normalized representation
/// Clause: C.6, 4.5.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_9003_a_query_answers_the_same_names_as_a_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/coercing.jsonld")?))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
        .entity_type(vehicle_type.clone())
        .property("testedAt", "2018-12-04T12:00:00Z")
        .property("brandName", "Mercedes")
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_value("testedAt", "2018-12-04T12:00:00Z")));
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            has_attribute(
                &format!("http://example.org/{namespace}/testedAt"),
                any_attribute().value("2018-12-04T12:00:00Z")
            )
        )
    );
    Ok(())
}
