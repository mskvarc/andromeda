//! A relationship to an entity at the bound is compared and followed.

use andromeda_test_suite::{broker, exact_length::padded, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, EntityId, NgsiLdMediaType},
    query::{Join, Locality, q},
};

/// Pins: A Relationship whose object is the id of an Entity at the 1,024-octet bound is selected by a q equality on
/// that object, and a q hop link{name}=="x" with join inline through it selects the source
/// Clause: 4.9, 4.5.3, 5.7.2.4, 6.23.3.1
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9005_a_relationship_to_an_entity_at_the_bound_is_compared_and_followed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let long_reference_type = namespace.entity_type("LongReference")?;
    let target_text = padded(&namespace.urn_text("LongReference", "target:"), 1024)?;
    let target_id = target_text.parse::<EntityId>()?;
    let source_id = namespace.urn("LongReference", "link-source")?;
    let target = Entity::builder()
        .id(target_id.clone())
        .entity_type(long_reference_type.clone())
        .property("name", "x")
        .build()?;
    broker.entities().create(&target).send().await?.done()?;
    let source = Entity::builder()
        .id(source_id.clone())
        .entity_type(long_reference_type.clone())
        .relationship("link", target_id)
        .build()?;
    broker.entities().create(&source).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&long_reference_type)
        .q(q::attr("link").eq(target_text.as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&source_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&long_reference_type)
        .locality(Locality::Local)
        .q(q::attr("link").linked("name").eq("x"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&source_id)));
    Ok(())
}
