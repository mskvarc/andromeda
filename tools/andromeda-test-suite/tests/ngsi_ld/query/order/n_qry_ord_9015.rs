//! The icu root collation identifier is applied.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: `orderBy=name` with collation=und-x-icu orders Schafer, Schäfer, Schaffer
/// Clause: 4.23.1, 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9015_the_icu_root_collation_identifier_is_applied() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_schaefer_id = namespace.urn("Building", "schaefer")?;
    let building_type = namespace.entity_type("Building")?;
    let building_schafer_id = namespace.urn("Building", "schafer")?;
    let building_schaffer_id = namespace.urn("Building", "schaffer")?;
    let entities = [
        Entity::builder()
            .id(building_schaefer_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Schäfer")
            .build()?,
        Entity::builder()
            .id(building_schafer_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Schafer")
            .build()?,
        Entity::builder()
            .id(building_schaffer_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Schaffer")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_schaefer_id.clone(), building_schafer_id.clone(), building_schaffer_id.clone()])
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("name".parse::<OrderBy>()?).collation("und-x-icu"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_schafer_id)));
    expect_that!(page.items(), nth(1, has_id(&building_schaefer_id)));
    expect_that!(page.items(), nth(2, has_id(&building_schaffer_id)));
    Ok(())
}
