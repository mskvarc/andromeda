//! The reified representations keep the full uri of a coerced term.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, Entity, Lossless, NgsiLdMediaType},
};

/// Pins: Retrieve Entity under a @context coercing `testedAt` to `DateTime`, by default and with format=normalized
/// or format=concise, answers `testedAt` under its full URI, not its short name, beside the plain `brandName` term,
/// and normalized as a Property of the instant
/// Clause: C.6, 4.5.1, 5.5.7, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_9002_the_reified_representations_keep_the_full_uri_of_a_coerced_term() -> Result<()> {
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
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("testedAt"));
    expect_that!(entity, has_attribute(&format!("http://example.org/{namespace}/testedAt"), any_attribute()));
    expect_that!(entity, has_attribute("brandName", any_attribute()));
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .form(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().explicit_normalized())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("testedAt"));
    expect_that!(entity_2, has_attribute(&format!("http://example.org/{namespace}/testedAt"), any_attribute()));
    expect_that!(entity_2, has_attribute("brandName", any_attribute()));
    let retrieved_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .form(Lossless::Concise)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, lacks_attribute("testedAt"));
    expect_that!(entity_3, has_attribute(&format!("http://example.org/{namespace}/testedAt"), any_attribute()));
    expect_that!(entity_3, has_attribute("brandName", any_attribute()));
    let retrieved_4 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .form(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().explicit_normalized())
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(
        entity_4,
        has_attribute(&format!("http://example.org/{namespace}/testedAt"), property().value("2018-12-04T12:00:00Z"))
    );
    Ok(())
}
