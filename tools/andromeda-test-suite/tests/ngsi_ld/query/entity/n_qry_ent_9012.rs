//! A query neither lists nor counts an entity past its expires at.

use andromeda_test_suite::{broker, identity_matcher::has_id, instant_ahead::minutes_ahead, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities by type with `count=true`, by `GET /entities` and by `POST /entityOperations/query`, right
/// after creating one Entity whose `expiresAt` passed a minute ago and one without an `expiresAt`, lists the second
/// alone and counts 1
/// Clause: 4.22, 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_ent_9012_a_query_neither_lists_nor_counts_an_entity_past_its_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let alert_type = namespace.entity_type("Alert")?;
    let alert_expired_id = namespace.urn("Alert", "expired")?;
    let alert_standing_id = namespace.urn("Alert", "standing")?;
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let alert_expired = Entity::builder()
        .id(alert_expired_id)
        .entity_type(alert_type.clone())
        .property("severity", "high")
        .expires_at(a_minute_ago)
        .build()?;
    broker.entities().create(&alert_expired).send().await?.done()?;
    let alert_standing = Entity::builder()
        .id(alert_standing_id.clone())
        .entity_type(alert_type.clone())
        .property("severity", "low")
        .build()?;
    broker.entities().create(&alert_standing).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for method in [QueryMethod::Get, QueryMethod::Post] {
        let page = json
            .entities()
            .query()
            .method(method)
            .entity_type(&alert_type)
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(page.items(), elements_are![has_id(&alert_standing_id)], "{method:?}");
        expect_that!(page.total(), some(eq(1)), "{method:?}");
    }
    Ok(())
}
