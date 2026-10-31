//! A pattern with a language reads the language map whatever lang asks for.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{language_filter::LanguageFilter, q},
};

/// Pins: q street`[nl]`~="Grote.*" selects the entity whose nl string matches the pattern whether the request
/// carries lang=fr, lang=nl or none, and the answer is rendered in the language lang asks for; an entity whose only
/// entry is @none is not selected by the nl pattern
/// Clause: 4.9, 4.15, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4006_a_pattern_with_a_language_reads_the_language_map_whatever_lang_asks_for() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let place_market_id = namespace.urn("Place", "market")?;
    let place_type = namespace.entity_type("Place")?;
    let place_default_id = namespace.urn("Place", "default")?;
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
            .id(place_default_id)
            .entity_type(place_type.clone())
            .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("@none", "Grote Default")])?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&place_type)
        .q(q::attr("street").member("nl").matches("\"Grote.*\""))
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&place_market_id),
                has_attribute("street", any_attribute().value("Grand Place").lang("fr"))
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&place_type)
        .q(q::attr("street").member("nl").matches("\"Grote.*\""))
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            all!(
                has_id(&place_market_id),
                has_attribute("street", any_attribute().value("Grote Markt").lang("nl"))
            )
        )
    );
    Ok(())
}
