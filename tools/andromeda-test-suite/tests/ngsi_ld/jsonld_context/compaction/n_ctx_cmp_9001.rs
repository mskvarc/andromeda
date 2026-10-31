//! A coerced term names the attribute in the simplified representation.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
    simplified_matcher::{has_value, lacks_value},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity under a @context coercing `testedAt` to `DateTime`, with format=simplified,
/// `format=keyValues` or `options=keyValues`, answers `testedAt` by its short name with the bare instant, no full
/// URI beside it, and the plain `brandName` term
/// Clause: C.6, 4.5.4, 5.5.7, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_9001_a_coerced_term_names_the_attribute_in_the_simplified_representation() -> Result<()> {
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
        .entity_type(vehicle_type)
        .property("testedAt", "2018-12-04T12:00:00Z")
        .property("brandName", "Mercedes")
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_value("testedAt", "2018-12-04T12:00:00Z"));
    expect_that!(entity, lacks_value(&format!("http://example.org/{namespace}/testedAt")));
    expect_that!(entity, has_value("brandName", "Mercedes"));
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .spelling(RepresentationSpelling::default().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("testedAt", "2018-12-04T12:00:00Z"));
    expect_that!(entity_2, lacks_value(&format!("http://example.org/{namespace}/testedAt")));
    expect_that!(entity_2, has_value("brandName", "Mercedes"));
    let retrieved_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_value("testedAt", "2018-12-04T12:00:00Z"));
    expect_that!(entity_3, lacks_value(&format!("http://example.org/{namespace}/testedAt")));
    expect_that!(entity_3, has_value("brandName", "Mercedes"));
    Ok(())
}
