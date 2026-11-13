//! Geo properties rank first and entities lacking the attribute last.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: a distance ordering ranks the entities whose attribute is a `GeoProperty` first (nearest or farthest first
/// by direction), then a non-geo value of that attribute, then the entities lacking it, which are ranked last and
/// never dropped
/// Clause: 4.23.2, 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_0011_geo_properties_rank_first_and_entities_lacking_the_attribute_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ids = [
        namespace.urn("Building", 1)?,
        namespace.urn("Building", 2)?,
        namespace.urn("Building", 3)?,
        namespace.urn("Building", 4)?,
    ];
    let building_type = namespace.entity_type("Building")?;
    // `site` rather than `location`: Table 5.2.4-1 lets `location` hold a GeoProperty only, and a non-geo value of
    // the ordered attribute is one of the cases ranked.
    let entities = [
        Entity::builder()
            .id(ids[0].clone())
            .entity_type(building_type.clone())
            .geo_property("site", Shape::Point(Position::try_new(2.3522, 48.8566)?))
            .build()?,
        Entity::builder()
            .id(ids[1].clone())
            .entity_type(building_type.clone())
            .property("name", "NoSite")
            .build()?,
        Entity::builder()
            .id(ids[2].clone())
            .entity_type(building_type.clone())
            .property("site", "somewhere")
            .build()?,
        Entity::builder()
            .id(ids[3].clone())
            .entity_type(building_type.clone())
            .geo_property("site", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    for (direction, order) in [("asc", [3, 0, 2, 1]), ("desc", [0, 3, 2, 1])] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&building_type)
            .locality(Locality::Local)
            .ordering(
                EntityOrderingBuilder::from(format!("site;dist-{direction}").parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)),
            )
            .send()
            .await?;
        expect_that!(page.items(), len(eq(4)), "dist-{direction}");
        for (position, index) in order.into_iter().enumerate() {
            expect_that!(page.items(), nth(position, has_id(&ids[index])), "dist-{direction} at {position}");
        }
    }
    Ok(())
}
