//! A sub attribute language property without a language reads every language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q door.label=="Hinten" matches the entity one of whose label strings is Hinten when no language is named
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0014_a_sub_attribute_language_property_without_a_language_reads_every_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_front_id = namespace.urn("Building", "front")?;
    let building_type = namespace.entity_type("Building")?;
    let building_back_id = namespace.urn("Building", "back")?;
    let building_front = Entity::builder()
        .id(building_front_id)
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Front"), ("de", "Vorne")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_front).send().await?.done()?;
    let building_back = Entity::builder()
        .id(building_back_id.clone())
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Back"), ("de", "Hinten")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_back).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").eq("Hinten"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_back_id)));
    Ok(())
}
