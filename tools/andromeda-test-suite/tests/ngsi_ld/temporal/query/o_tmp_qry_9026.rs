//! The post binding points at no page it cannot serve.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A POST temporal query with limit 2 over three Entities answers two with NGSILD-Results-Count 3 and no next
/// or prev Link, and with offset 2 answers the third with neither
/// Clause: 4.12, 5.5.9.1, 6.3.10, 6.3.13, 6.24.3.1
/// Recorded: 5.5.9.1 wants pointers that carry every parameter of the next page, and the query of this binding is
/// its body, which no URI-reference carries; Andromeda answers no pointer and flags the remainder with the count
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9026_the_post_binding_points_at_no_page_it_cannot_serve() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_postpage_1_id = namespace.urn("Vehicle", "postpage-1")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_postpage_2_id = namespace.urn("Vehicle", "postpage-2")?;
    let vehicle_postpage_3_id = namespace.urn("Vehicle", "postpage-3")?;
    let vehicle_postpage_1_evolution = TemporalEntity::builder()
        .id(vehicle_postpage_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_postpage_1_evolution).send().await?.created()?;
    let vehicle_postpage_2_evolution = TemporalEntity::builder()
        .id(vehicle_postpage_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_postpage_2_evolution).send().await?.created()?;
    let vehicle_postpage_3_evolution = TemporalEntity::builder()
        .id(vehicle_postpage_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_postpage_3_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(vec![
            vehicle_postpage_1_id.clone(),
            vehicle_postpage_2_id.clone(),
            vehicle_postpage_3_id.clone(),
        ])
        .limit(2)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.next(), none());
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(3)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(vec![
            vehicle_postpage_1_id.clone(),
            vehicle_postpage_2_id.clone(),
            vehicle_postpage_3_id.clone(),
        ])
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_2.next(), none());
    expect_that!(page_2.prev(), none());
    expect_that!(page_2.items(), len(eq(1)));
    Ok(())
}
