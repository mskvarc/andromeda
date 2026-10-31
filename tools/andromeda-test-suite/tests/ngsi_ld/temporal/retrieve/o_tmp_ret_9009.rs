//! A temporal pick or omit leaving no member is not found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        Property,
        Relationship,
        geometry::{Position, Shape},
    },
    query::{
        projection::Projection,
        temporal_query::{self, TemporalProperty},
    },
};

/// Pins: Retrieve Temporal Evolution of an Entity with pick=unknown or an omit naming every member answers 404
/// `ResourceNotFound`, with pick=id,name answers 200, and Query Temporal Evolution of Entities with pick=unknown answers
/// an empty array
/// Clause: 5.7.3.4, 5.7.4.4, 4.21
/// Recorded: 5.7.1.4 raises `ResourceNotFound` when the Entity holds none of the Attributes in attrs, which Table
/// 6.5.3.1-1 makes a synonym for pick; whether a pick or an omit that leaves the Entity no member is the same failure is
/// left open. Andromeda answers 404 `ResourceNotFound`, on the temporal retrieval too, while a query leaves such an Entity
/// out
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9009_a_temporal_pick_or_omit_leaving_no_member_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_temporal_projection_id = namespace.urn("Building", "temporal-projection")?;
    let building_type = namespace.entity_type("Building")?;
    let city_temporal_projection_id = namespace.urn("City", "temporal-projection")?;
    let building_temporal_projection = Entity::builder()
        .id(building_temporal_projection_id.clone())
        .entity_type(building_type.clone())
        .property("name", Property::builder("Depot"))
        .relationship("linkedTo", Relationship::builder(city_temporal_projection_id))
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_temporal_projection).send().await?.done()?;
    let window = || -> Result<_> { Ok(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt)) };
    let client = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let unknown = client
        .temporal()
        .retrieve_projected(&building_temporal_projection_id)
        .temporal_q(window()?)
        .pick("unknown".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let emptied = client
        .temporal()
        .retrieve_projected(&building_temporal_projection_id)
        .temporal_q(window()?)
        .omit("id,type,name,linkedTo,location".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&emptied, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let picked = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_projected(&building_temporal_projection_id)
        .temporal_q(window()?)
        .pick("id,name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(picked.body().id.as_ref(), some(eq(&building_temporal_projection_id)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_projected(window()?)
        .entity_type(&building_type)
        .ids(vec![building_temporal_projection_id])
        .pick("unknown".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    Ok(())
}
