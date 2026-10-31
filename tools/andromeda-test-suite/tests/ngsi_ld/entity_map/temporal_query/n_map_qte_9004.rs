//! A post follow up naming an entity outside the map answers nothing.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A POST temporal query naming an Entity the map does not list answers it without the map and answers an
/// empty array under the map's `NGSILD-EntityMap`
/// Clause: 5.5.14, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_map_qte_9004_a_post_follow_up_naming_an_entity_outside_the_map_answers_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let outside_1_id = namespace.urn("Vehicle", "outside-1")?;
    let outside_2_id = namespace.urn("Vehicle", "outside-2")?;
    let outside_absent_id = namespace.urn("Vehicle", "outside-absent")?;
    for id in [&outside_1_id, &outside_2_id, &outside_absent_id] {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
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
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .ids(vec![outside_1_id, outside_2_id])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let outside_page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(vec![outside_absent_id.clone()])
        .send()
        .await?;
    expect_that!(outside_page.items(), len(eq(1)));
    expect_that!(outside_page.items(), contains(has_id(&outside_absent_id)));
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let mapped_page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request_2)
        .entity_type(&vehicle_type)
        .ids(vec![outside_absent_id])
        .send()
        .await?;
    expect_that!(mapped_page.items(), is_empty());
    Ok(())
}
