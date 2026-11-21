//! An id pattern matches anywhere in the identifier unless anchored.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::IdPattern,
};

/// Pins: Query Entities by type and an `idPattern` selects every Entity whose identifier the pattern is found in:
/// over `urn:ngsi-ld:Car:<ns>-1` and `urn:ngsi-ld:Car:<ns>-12`, `Car:<ns>-1` selects both, `Car:<ns>-1$` selects
/// only the first, and `^urn:ngsi-ld:Car:<ns>-1` selects both
/// Clause: 5.7.2.4, 6.4.3.2
///
/// The pattern is an IEEE 1003.2 regular expression, whose matching searches a string for a matching sequence, and
/// `^` and `$` limit the match to the start and the end of the identifier.
#[gtest]
#[tokio::test]
async fn n_qry_ent_9018_an_id_pattern_matches_anywhere_in_the_identifier_unless_anchored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let car_1_id = namespace.urn("Car", 1)?;
    let car_12_id = namespace.urn("Car", 12)?;
    let entities = [
        Entity::builder().id(car_1_id.clone()).entity_type(car_type.clone()).build()?,
        Entity::builder().id(car_12_id.clone()).entity_type(car_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .id_pattern(format!("Car:{namespace}-1").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&car_1_id)));
    expect_that!(page.items(), contains(has_id(&car_12_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .id_pattern(format!("Car:{namespace}-1$").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&car_1_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .id_pattern(format!("^urn:ngsi-ld:Car:{namespace}-1").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&car_1_id)));
    expect_that!(page_3.items(), contains(has_id(&car_12_id)));
    Ok(())
}
