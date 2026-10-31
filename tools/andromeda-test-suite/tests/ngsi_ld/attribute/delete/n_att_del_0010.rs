//! Scope is removed from the entity.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Entity, NgsiLdMediaType},
};

/// Pins: Delete Attribute addressed to scope answers 204 and the Entity no longer has a scope
/// Clause: 5.6.5.4, 4.18, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0010_scope_is_removed_from_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The model refuses `scope` as an attribute name, so the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &DeleteAttribute::builder()
                .id(&building_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{building_id}/attrs/scope"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope, none());
    Ok(())
}
