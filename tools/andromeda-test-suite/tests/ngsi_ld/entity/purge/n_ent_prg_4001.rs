//! Purging scope closes its temporal interval.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Purge Entities with drop=scope removes the scope, and the Temporal Evolution then holds a deleted scope
/// instance, as it does for a purged Property
/// Clause: 4.18, 5.6.21, 4.5.7
/// Recorded: 4.18 makes a Scope valid "from the time it has been set until the time it has been explicitly removed
/// by an update or delete operation"; a purge that removes it is such an operation, and drop=name on the same kind
/// of entity records the deletion of name
#[gtest]
#[tokio::test]
async fn n_ent_prg_4001_purging_scope_closes_its_temporal_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/a/b")
        .property("name", "Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // An attribute name cannot be `scope`, so the drop list is sent as written.
    let request = request_to(&broker, Method::DELETE, &format!("entities?type=Building_{namespace}&drop=scope"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope, none());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution.scope, not(is_empty()));
    expect_that!(evolution.scope.iter().any(|instance| instance.output().deleted_at.is_some()), eq(true));
    Ok(())
}
