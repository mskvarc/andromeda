//! A temporal q on a compound value member.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with q=address`[city]`=="Bern" selects the Building whose address
/// instance holds the city Bern and not the one holding Zurich
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9058_a_temporal_q_on_a_compound_value_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            Property::builder(object([("city", Value::from("Bern")), ("street", Value::from("Marktgasse"))])).observed_at("2020-01-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let building_2_evolution = TemporalEntity::builder()
        .id(building_2_id)
        .entity_type(building_type.clone())
        .property(
            "address",
            Property::builder(object([("city", Value::from("Zurich")), ("street", Value::from("Bahnhofstrasse"))]))
                .observed_at("2020-01-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_2_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("address").member("city").eq("Bern"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
