//! A language property sub attribute matches each of its texts.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q equal to a string on a `LanguageProperty` sub-attribute selects the Entity when the
/// text of any language equals it
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9062_a_language_property_sub_attribute_matches_each_of_its_texts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gate_id = namespace.urn("Gate", 1)?;
    let gate_type = namespace.entity_type("Gate")?;
    let gate_2_id = namespace.urn("Gate", 2)?;
    let gate = Entity::builder()
        .id(gate_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "gate",
            Property::builder("open").language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "North gate"), ("de", "Nordtor")])?),
            ),
        )
        .build()?;
    broker.entities().create(&gate).send().await?.done()?;
    let gate_2 = Entity::builder()
        .id(gate_2_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "gate",
            Property::builder("open").language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "South gate"), ("de", "Suedtor")])?),
            ),
        )
        .build()?;
    broker.entities().create(&gate_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("label").eq("Nordtor"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&gate_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("label").eq("South gate"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&gate_2_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&gate_type)
        .q(q::attr("gate").sub_attr("label").eq("West gate"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
