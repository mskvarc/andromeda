//! A query naming an entity past its expires at returns an empty array.

use andromeda_test_suite::{broker, instant_ahead::minutes_ahead, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities naming by `id` an Entity whose `expiresAt` passed a minute ago, by `GET /entities` and by
/// `POST /entityOperations/query`, right after creating it, answers 200 with an empty array
/// Clause: 4.22, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_9013_a_query_naming_an_entity_past_its_expires_at_returns_an_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let alert_type = namespace.entity_type("Alert")?;
    let alert_expired_id = namespace.urn("Alert", "expired")?;
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let alert_expired = Entity::builder()
        .id(alert_expired_id.clone())
        .entity_type(alert_type.clone())
        .property("severity", "high")
        .expires_at(a_minute_ago)
        .build()?;
    broker.entities().create(&alert_expired).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for method in [QueryMethod::Get, QueryMethod::Post] {
        let page = json
            .entities()
            .query()
            .method(method)
            .entity_type(&alert_type)
            .ids(vec![alert_expired_id.clone()])
            .send()
            .await?;
        expect_that!(page.items(), is_empty(), "{method:?}");
    }
    Ok(())
}
