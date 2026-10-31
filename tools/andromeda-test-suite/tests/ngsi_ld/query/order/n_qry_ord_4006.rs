//! A second ordering term breaks a tie among entities without a scope.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: orderBy=scope,name ranks the entities without a scope after every entity with one, and orders the ones
/// without a scope, and the ones that share a scope, by name
/// Clause: 4.23.2, 4.23.3, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_4006_a_second_ordering_term_breaks_a_tie_among_entities_without_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_4_id = namespace.urn("Building", 4)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Delta")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Alpha")
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .scope("/a")
            .property("name", "Zulu")
            .build()?,
        Entity::builder()
            .id(building_4_id.clone())
            .entity_type(building_type.clone())
            .scope("/a")
            .property("name", "Mike")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("scope,name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), nth(0, has_id(&building_4_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_2_id)));
    expect_that!(page.items(), nth(3, has_id(&building_id)));
    Ok(())
}
