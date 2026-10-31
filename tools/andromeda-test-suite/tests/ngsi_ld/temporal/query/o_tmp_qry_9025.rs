//! An order by without local scope still orders the answer.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{ordering::OrderBy, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over Entities no registration covers, without local scope, answers
/// orderBy=id in ICU root order and orderBy=id;desc in its reverse on the GET and the POST binding
/// Clause: 4.23.1, 5.5.13, 5.7.4.4
/// Recorded: 5.7.2.4 and 5.7.4.4 refuse an ordering whose execution is not limited to the local scope (5.5.13), which
/// 5.5.13 defines as using no Context Source Registration; whether a query no registration matches is in that state
/// without local=true is left open. Andromeda admits the ordering when no registration takes part in the query
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9025_an_order_by_without_local_scope_still_orders_the_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_collation2_id = namespace.urn("CollationReading", "collation2")?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let collation_reading_collation10_id = namespace.urn("CollationReading", "collation10")?;
    let collation_reading_collation30_id = namespace.urn("CollationReading", "collation30")?;
    for id in [
        &collation_reading_collation2_id,
        &collation_reading_collation10_id,
        &collation_reading_collation30_id,
    ] {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(collation_reading_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let ids = vec![
        collation_reading_collation2_id.clone(),
        collation_reading_collation10_id.clone(),
        collation_reading_collation30_id.clone(),
    ];
    let ascending = [
        collation_reading_collation10_id.clone(),
        collation_reading_collation2_id.clone(),
        collation_reading_collation30_id.clone(),
    ];
    let descending = [
        collation_reading_collation30_id,
        collation_reading_collation2_id,
        collation_reading_collation10_id,
    ];
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let page = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .ids(ids.clone())
        .ordering("id".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), ids_in_order(&ascending));
    let page_2 = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .ids(ids.clone())
        .ordering("id;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), ids_in_order(&descending));
    let page_3 = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&collation_reading_type)
        .ids(ids)
        .ordering("id;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), ids_in_order(&descending));
    Ok(())
}
