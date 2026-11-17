//! Near gives a polygon the same answer whether it is the only located entity of the tenant or one of a thousand.

use andromeda_test_suite::{
    broker,
    geometry_round_trip::{polygon, rectangle},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// How many Zones are located inside the reference square beside the polygon.
const OTHERS: u32 = 1000;

/// Pins: in a tenant whose only Zone is located at the polygon (170,-10), (-170,-10), (-170,10), (170,10), Query
/// Entities with near;maxDistance==1000 around the square (0,0) to (10,10) answers no Zone and with
/// near;minDistance==1000 around it answers that Zone; once 1 000 more Zones are located at points inside the square,
/// the first query answers those 1 000 and the second that Zone alone; the polygon is the one 20 degrees wide around
/// the antimeridian, 16 890 km from the square, whose corner (10,10) is the antipode of the polygon's corner
/// (-170,-10)
/// Clause: 4.10, 5.7.2.4
/// Tenant: private (the first two queries must measure the polygon as the only located Entity of the tenant, which
/// the shared tenant's other files would locate Entities beside)
#[gtest]
#[tokio::test]
async fn n_qry_geo_9098_near_gives_a_polygon_the_same_answer_whether_it_is_the_only_located_entity_of_the_tenant_or_one_of_a_thousand() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let zone_type = namespace.entity_type("Zone")?;
    let antimeridian_id = namespace.urn("Zone", "antimeridian")?;
    let antimeridian = Entity::builder()
        .id(antimeridian_id.clone())
        .entity_type(zone_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(polygon(&[&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]])?),
        )
        .build()?;
    tenant.entities().create(&antimeridian).send().await?.done()?;
    let near = geoquery::near_max(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let beyond = geoquery::near_min(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?;
    let alone_near = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&zone_type)
        .geo_q(near.clone())
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(alone_near.total(), some(eq(0)));
    let alone_beyond = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&zone_type)
        .geo_q(beyond.clone())
        .send()
        .await?;
    expect_that!(alone_beyond.items(), len(eq(1)));
    expect_that!(alone_beyond.items(), nth(0, has_id(&antimeridian_id)));
    let mut others = Vec::new();
    for index in 0..OTHERS {
        others.push(
            Entity::builder()
                .id(namespace.urn("Zone", format!("inside-{index}"))?)
                .entity_type(zone_type.clone())
                .geo_property(
                    "location",
                    Shape::Point(Position::try_new(0.5 + f64::from(index % 40) * 0.2, 0.5 + f64::from(index / 40) * 0.3)?),
                )
                .build()?,
        );
    }
    for batch in others.chunks(100) {
        tenant.entities().batch_create(batch).send().await?.done()?;
    }
    let among_others_near = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&zone_type)
        .geo_q(near)
        .limit(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(among_others_near.total(), some(eq(u64::from(OTHERS))));
    let among_others_beyond = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&zone_type)
        .geo_q(beyond)
        .send()
        .await?;
    expect_that!(among_others_beyond.items(), len(eq(1)));
    expect_that!(among_others_beyond.items(), nth(0, has_id(&antimeridian_id)));
    Ok(())
}
