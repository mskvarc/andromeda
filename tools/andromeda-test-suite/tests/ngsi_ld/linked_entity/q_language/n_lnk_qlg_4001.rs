//! A hop pattern with a language reads the language of the linked entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: q isLocatedAt{street`[nl]`}~="Grote.*" selects the root whose linked entity holds a nl string matching the
/// pattern, and not the root whose linked entity holds only a fr entry
/// Clause: 4.9, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_4001_a_hop_pattern_with_a_language_reads_the_language_of_the_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let place_market_id = namespace.urn("Place", "market")?;
    let place_type = namespace.entity_type("Place")?;
    let place_french_id = namespace.urn("Place", "french")?;
    let shop_id = namespace.urn("Shop", 1)?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_2_id = namespace.urn("Shop", 2)?;
    let entities = [
        Entity::builder()
            .id(place_market_id.clone())
            .entity_type(place_type.clone())
            .language_property(
                "street",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("nl", "Grote Markt"), ("fr", "Grand Place")])?),
            )
            .build()?,
        Entity::builder()
            .id(place_french_id.clone())
            .entity_type(place_type.clone())
            .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grote Place")])?))
            .build()?,
        Entity::builder()
            .id(shop_id.clone())
            .entity_type(shop_type.clone())
            .relationship("isLocatedAt", place_market_id.clone())
            .build()?,
        Entity::builder()
            .id(shop_2_id)
            .entity_type(shop_type.clone())
            .relationship("isLocatedAt", place_french_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shop_type)
        .q(q::attr("isLocatedAt").linked("street").member("nl").matches("\"Grote.*\""))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&shop_id)));
    Ok(())
}
