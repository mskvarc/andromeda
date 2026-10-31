//! A post query pattern with a language reads only that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST with q street`[nl]`~="Grote.*" selects the entity whose nl string matches the
/// pattern, and street`[fr]`~="Grote.*" selects none, an entity with only a fr entry never matching the nl pattern
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4005_a_post_query_pattern_with_a_language_reads_only_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let place_both_id = namespace.urn("Place", "both")?;
    let place_type = namespace.entity_type("Place")?;
    let place_french_id = namespace.urn("Place", "french")?;
    let entities = [
        Entity::builder()
            .id(place_both_id.clone())
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
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&place_type)
        .q(q::attr("street").member("nl").matches("\"Grote.*\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&place_both_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&place_type)
        .q(q::attr("street").member("fr").matches("\"Grote.*\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&place_french_id)));
    Ok(())
}
