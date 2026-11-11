//! A posted temporal query whose result fits one page points at no other page.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` with limit 3 over two
/// Entities answers both and neither a next nor a prev Link
/// Clause: 5.5.9.1, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9094_a_posted_temporal_query_whose_result_fits_one_page_points_at_no_other_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let ids = vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?];
    for id in &ids {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property(
                "speed",
                Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
            )
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(ids.clone())
        .limit(3)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.next(), none());
    expect_that!(page.prev(), none());
    Ok(())
}
