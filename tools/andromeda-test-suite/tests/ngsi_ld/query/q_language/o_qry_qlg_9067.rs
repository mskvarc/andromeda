//! An unquoted pattern matches as the quoted one, metacharacters included.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: a pattern written without quotes selects what the same pattern in quotes selects under ~= and !~=, a bare
/// `^Merc` and a pattern made of letters, hyphens and colons alike
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9067_an_unquoted_pattern_matches_as_the_quoted_one_metacharacters_included() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_mercedes_id = namespace.urn("Car", "mercedes")?;
    let car_type = namespace.entity_type("Car")?;
    let car_mercedes_benz_id = namespace.urn("Car", "mercedes-benz")?;
    let car_volvo_id = namespace.urn("Car", "volvo")?;
    let entities = [
        Entity::builder()
            .id(car_mercedes_id.clone())
            .entity_type(car_type.clone())
            .property("brandName", "Mercedes")
            .build()?,
        Entity::builder()
            .id(car_mercedes_benz_id.clone())
            .entity_type(car_type.clone())
            .property("brandName", "Mercedes-Benz:AMG")
            .build()?,
        Entity::builder()
            .id(car_volvo_id.clone())
            .entity_type(car_type.clone())
            .property("brandName", "Volvo")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("Merc"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&car_mercedes_id)));
    expect_that!(page.items(), contains(has_id(&car_mercedes_benz_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("\"Merc\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&car_mercedes_id)));
    expect_that!(page_2.items(), contains(has_id(&car_mercedes_benz_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("Benz:AMG"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&car_mercedes_benz_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").not_matches("Merc"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&car_volvo_id)));
    let page_bare_anchored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("^Merc"))
        .send()
        .await?;
    expect_that!(page_bare_anchored.items(), len(eq(2)));
    expect_that!(page_bare_anchored.items(), contains(has_id(&car_mercedes_id)));
    expect_that!(page_bare_anchored.items(), contains(has_id(&car_mercedes_benz_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("\"^Merc\""))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), contains(has_id(&car_mercedes_id)));
    expect_that!(page_5.items(), contains(has_id(&car_mercedes_benz_id)));
    Ok(())
}
